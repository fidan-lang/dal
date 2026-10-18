use std::sync::Arc;
use std::time::{Duration, Instant};

use jsonwebtoken::{
    Algorithm, DecodingKey, Validation, decode, decode_header,
    jwk::{AlgorithmParameters, JwkSet},
};
use serde::{Deserialize, Serialize};
use tokio::sync::{Mutex, RwLock};
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
/// JWKS is cached for at most one hour; unknown keys trigger a refresh,
/// at most once per five seconds (rotation without attacker-triggered fetch floods).
#[derive(Clone)]
pub struct JwtValidator {
    inner: Arc<Inner>,
}

struct Inner {
    jwks_uri: String,
    issuer: String,
    audience: String,
    /// Cached JwkSet, refreshed lazily on unknown `kid`.
    cache: RwLock<Option<(JwkSet, Instant)>>,
    refresh: Mutex<()>,
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
                refresh: Mutex::new(()),
                http: reqwest::Client::builder()
                    .timeout(Duration::from_secs(10))
                    .build()
                    .expect("build reqwest client"),
            }),
        }
    }

    /// Validate a raw JWT string and return the decoded claims.
    pub async fn validate(&self, token: &str) -> Result<Claims, DalError> {
        if token.len() > 16 * 1024 {
            return Err(DalError::Unauthorized);
        }
        let header = decode_header(token).map_err(|_| DalError::Unauthorized)?;
        if header.alg != Algorithm::RS256 {
            return Err(DalError::Unauthorized);
        }
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

        validate_claims(&data.claims, &self.inner.issuer, &self.inner.audience)?;
        Ok(data.claims)
    }

    async fn find_key(&self, kid: &str) -> Option<DecodingKey> {
        let cache = self.inner.cache.read().await;
        let (jwks, fetched) = cache.as_ref()?;
        if fetched.elapsed() >= Duration::from_secs(3600) {
            return None;
        }
        let jwk = jwks.find(kid)?;
        match &jwk.algorithm {
            AlgorithmParameters::RSA(rsa) => DecodingKey::from_rsa_components(&rsa.n, &rsa.e).ok(),
            _ => None,
        }
    }

    async fn refresh_jwks(&self) -> Result<(), DalError> {
        // Serialize refreshes and bound unknown-kid traffic; keys expire after one hour.
        let _refresh = self.inner.refresh.lock().await;
        if self
            .inner
            .cache
            .read()
            .await
            .as_ref()
            .is_some_and(|(_, at)| at.elapsed() < Duration::from_secs(5))
        {
            return Ok(());
        }
        debug!("Refreshing Cognito JWKS");
        let mut resp = self
            .inner
            .http
            .get(&self.inner.jwks_uri)
            .send()
            .await
            .map_err(|e| DalError::Cognito(e.to_string()))?;

        if !resp.status().is_success() {
            return Err(DalError::Cognito("JWKS endpoint failed".into()));
        }
        let mut bytes = Vec::new();
        while let Some(chunk) = resp
            .chunk()
            .await
            .map_err(|_| DalError::Cognito("JWKS read failed".into()))?
        {
            if bytes.len().saturating_add(chunk.len()) > 64 * 1024 {
                return Err(DalError::Cognito("JWKS exceeds size limit".into()));
            }
            bytes.extend_from_slice(&chunk);
        }
        let jwks: JwkSet =
            serde_json::from_slice(&bytes).map_err(|_| DalError::Cognito("invalid JWKS".into()))?;
        let mut cache = self.inner.cache.write().await;
        *cache = Some((jwks, Instant::now()));
        Ok(())
    }
}

fn validate_claims(claims: &Claims, issuer: &str, client: &str) -> Result<(), DalError> {
    // Cognito ID and access tokens have distinct audience claims. A claim for the
    // other token kind must not authorize a token with the wrong audience.
    let audience_matches = match claims.token_use.as_deref() {
        Some("id") => claims.aud.as_deref() == Some(client),
        Some("access") => claims.client_id.as_deref() == Some(client),
        _ => false,
    };
    if claims.iss != issuer || claims.sub.is_empty() || !audience_matches {
        return Err(DalError::Unauthorized);
    }
    Ok(())
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn cognito_token_kinds_have_distinct_audience_contracts() {
        let mut claims = Claims {
            sub: "subject".into(),
            cognito_username: None,
            email: None,
            email_verified: None,
            aud: Some("client".into()),
            client_id: None,
            iss: "issuer".into(),
            exp: 100,
            iat: 1,
            token_use: Some("id".into()),
        };
        assert!(validate_claims(&claims, "issuer", "client").is_ok());
        claims.aud = Some("other".into());
        claims.client_id = Some("client".into());
        assert!(validate_claims(&claims, "issuer", "client").is_err());
        claims.token_use = Some("access".into());
        assert!(validate_claims(&claims, "issuer", "client").is_ok());
        claims.client_id = Some("other".into());
        claims.aud = Some("client".into());
        assert!(validate_claims(&claims, "issuer", "client").is_err());
        claims.token_use = None;
        assert!(validate_claims(&claims, "issuer", "client").is_err());
        claims.token_use = Some("id".into());
        assert!(validate_claims(&claims, "wrong-issuer", "client").is_err());
        claims.sub.clear();
        assert!(validate_claims(&claims, "issuer", "client").is_err());
    }
}
