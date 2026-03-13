use std::sync::Arc;
use std::time::Duration;

use jsonwebtoken::{
    Algorithm, DecodingKey, Validation, decode, decode_header,
    jwk::{AlgorithmParameters, JwkSet},
};
use serde::{Deserialize, Serialize};
use tokio::sync::RwLock;
use tracing::debug;

use dal_common::error::DalError;

/// JWT claims issued by AWS Cognito (id_token).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Claims {
    /// Cognito `sub` (stable user identifier across password changes).
    pub sub: String,
    /// Cognito username.
    #[serde(rename = "cognito:username")]
    pub cognito_username: Option<String>,
    pub email: Option<String>,
    pub email_verified: Option<bool>,
    pub aud: Option<String>,
    pub client_id: Option<String>,
    pub iss: String,
    pub exp: i64,
    pub iat: i64,
    pub token_use: Option<String>,
}

/// Validates Cognito JWTs.
///
/// JWKS is fetched once and cached in memory; it is refreshed automatically
/// on any key-not-found error (handles Cognito rotating keys).
#[derive(Clone)]
pub struct JwtValidator {
    inner: Arc<Inner>,
}

struct Inner {
    jwks_uri: String,
    issuer: String,
    audience: String,
    /// Cached JwkSet, refreshed lazily on unknown `kid`.
    cache: RwLock<Option<JwkSet>>,
    http: reqwest::Client,
}

impl JwtValidator {
    /// Create a new validator.
    ///
    /// * `region`    – AWS region, e.g. `eu-central-1`
    /// * `pool_id`   – Cognito User Pool ID, e.g. `eu-central-1_Xyz`
    /// * `client_id` – App client ID (audience)
    pub fn new(region: &str, pool_id: &str, client_id: &str, endpoint_url: Option<&str>) -> Self {
        let issuer = match endpoint_url {
            Some(base) => format!("{}/{}", base.trim_end_matches('/'), pool_id),
            None => format!("https://cognito-idp.{region}.amazonaws.com/{pool_id}"),
        };
        let jwks_uri = format!("{issuer}/.well-known/jwks.json");
        Self {
            inner: Arc::new(Inner {
                jwks_uri,
                issuer,
                audience: client_id.to_string(),
                cache: RwLock::new(None),
                http: reqwest::Client::builder()
                    .timeout(Duration::from_secs(10))
                    .build()
                    .expect("build reqwest client"),
            }),
        }
    }

    /// Validate a raw JWT string and return the decoded claims.
    pub async fn validate(&self, token: &str) -> Result<Claims, DalError> {
        let header = decode_header(token).map_err(|_| DalError::Unauthorized)?;
        let kid = header.kid.ok_or(DalError::Unauthorized)?;

        // Try cached JWKS first
        let key = self.find_key(&kid).await;
        let key = match key {
            Some(k) => k,
            None => {
                // Refresh and retry once
                self.refresh_jwks().await?;
                self.find_key(&kid).await.ok_or(DalError::Unauthorized)?
            }
        };

        let mut validation = Validation::new(Algorithm::RS256);
        validation.validate_aud = false;

        let data =
            decode::<Claims>(token, &key, &validation).map_err(|_| DalError::Unauthorized)?;

        let claims = data.claims;
        if claims.iss != self.inner.issuer {
            return Err(DalError::Unauthorized);
        }

        let audience_matches = claims.aud.as_deref() == Some(self.inner.audience.as_str())
            || claims.client_id.as_deref() == Some(self.inner.audience.as_str());
        if !audience_matches {
            return Err(DalError::Unauthorized);
        }

        Ok(claims)
    }

    async fn find_key(&self, kid: &str) -> Option<DecodingKey> {
        let cache = self.inner.cache.read().await;
        let jwks = cache.as_ref()?;
        let jwk = jwks.find(kid)?;
        match &jwk.algorithm {
            AlgorithmParameters::RSA(rsa) => DecodingKey::from_rsa_components(&rsa.n, &rsa.e).ok(),
            _ => None,
        }
    }

    async fn refresh_jwks(&self) -> Result<(), DalError> {
        debug!("Refreshing Cognito JWKS from {}", self.inner.jwks_uri);
        let resp = self
            .inner
            .http
            .get(&self.inner.jwks_uri)
            .send()
            .await
            .map_err(|e| DalError::Cognito(e.to_string()))?;

        let jwks: JwkSet = resp
            .json()
            .await
            .map_err(|e| DalError::Cognito(format!("parse JWKS: {e}")))?;

        let mut cache = self.inner.cache.write().await;
        *cache = Some(jwks);
        Ok(())
    }
}
