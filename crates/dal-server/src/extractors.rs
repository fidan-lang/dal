use axum::{
    extract::FromRequestParts,
    http::{HeaderMap, header, request::Parts},
};
use dal_auth::Claims;
use dal_auth::has_scope;
use dal_common::error::DalError;
use dal_db::{
    models::{ApiToken, User},
    queries,
};

use crate::state::AppState;

/// Extractor: authenticated user from JWT (Bearer token in Authorization header)
/// OR the `dal_access_token` cookie for browser sessions.
/// API tokens are still accepted via `Authorization: Bearer dal_...`.
pub struct AuthUser(pub User);

/// Credential administration and session operations require a verified JWT session.
/// API credentials are rejected before token lookup/touch or any provider operation.
pub struct VerifiedSession(pub User);
pub struct AuthActor {
    pub user: User,
    pub api_token: Option<ApiToken>,
}

impl AuthActor {
    pub async fn transaction<'a>(
        &self,
        db: &'a dal_db::PgPool,
    ) -> Result<sqlx::Transaction<'a, sqlx::Postgres>, DalError> {
        Ok(dal_db::actor_transaction(
            db,
            self.user.id,
            self.api_token.as_ref().map(|token| token.id),
        )
        .await?)
    }

    pub fn is_admin(&self) -> bool {
        self.user.is_admin && self.user.email_verified && self.api_token.is_none()
    }

    pub fn require_admin(&self) -> Result<(), DalError> {
        if self.is_admin() {
            Ok(())
        } else {
            Err(DalError::Forbidden)
        }
    }

    pub fn require_scope(&self, scope: &str) -> Result<(), DalError> {
        if !self.user.email_verified {
            return Err(DalError::Forbidden);
        }
        if let Some(token) = &self.api_token
            && !has_scope(&token.scopes, scope)
        {
            return Err(DalError::Forbidden);
        }
        Ok(())
    }
}

impl FromRequestParts<AppState> for AuthUser {
    type Rejection = DalError;

    async fn from_request_parts(
        parts: &mut Parts,
        state: &AppState,
    ) -> Result<Self, Self::Rejection> {
        let actor = AuthActor::from_request_parts(parts, state).await?;
        Ok(AuthUser(actor.user))
    }
}

impl FromRequestParts<AppState> for VerifiedSession {
    type Rejection = DalError;

    async fn from_request_parts(
        parts: &mut Parts,
        state: &AppState,
    ) -> Result<Self, Self::Rejection> {
        let token = extract_auth_token(&parts.headers).ok_or(DalError::Unauthorized)?;
        // Session-only endpoints reject the API-credential class (403), including
        // expired/invalid API credentials, without updating usage or calling Cognito.
        if token.starts_with("dal_") {
            return Err(DalError::Forbidden);
        }
        let actor = AuthActor::from_request_parts(parts, state).await?;
        if !actor.user.email_verified {
            return Err(DalError::Forbidden);
        }
        Ok(Self(actor.user))
    }
}

impl FromRequestParts<AppState> for AuthActor {
    type Rejection = DalError;

    async fn from_request_parts(
        parts: &mut Parts,
        state: &AppState,
    ) -> Result<Self, Self::Rejection> {
        let token = extract_auth_token(&parts.headers).ok_or(DalError::Unauthorized)?;

        // Try API token first (fast path — starts with "dal_")
        if token.starts_with("dal_") {
            if !dal_auth::api_token::validate_api_token_format(&token) {
                return Err(DalError::InvalidApiToken);
            }
            let hash = dal_auth::hash_token(&token);
            let api_tok = queries::tokens::get_by_hash(&state.db, &hash).await?;
            let api_tok = api_tok.ok_or(DalError::Unauthorized)?;

            state
                .best_effort_accounting(queries::tokens::touch(&state.db, &hash))
                .await;

            let user = queries::users::auth_by_id(&state.db, api_tok.user_id).await?;
            let user = user.ok_or(DalError::Unauthorized)?;

            return Ok(AuthActor {
                user,
                api_token: Some(api_tok),
            });
        }

        let user = authenticated_session(state, &token).await?;

        Ok(AuthActor {
            user,
            api_token: None,
        })
    }
}

/// One JWT session contract for authorization, login and refresh responses.
pub(crate) async fn authenticated_session(state: &AppState, token: &str) -> Result<User, DalError> {
    let (user, _) = validated_identity(state, token).await?;
    if queries::users::mfa_required(&state.db, user.id).await?
        && !queries::mfa_sessions::authorized(&state.db, user.id, &dal_auth::hash_token(token))
            .await?
    {
        return Err(DalError::Unauthorized);
    }
    Ok(user)
}

/// Used only after fresh Cognito authentication or while binding a refresh to its MFA grant.
pub(crate) async fn validated_identity(
    state: &AppState,
    token: &str,
) -> Result<(User, Claims), DalError> {
    let claims: Claims = state.jwt.validate(token).await?;
    let user = queries::users::get_by_cognito_sub(&state.db, &claims.sub)
        .await?
        .ok_or(DalError::Unauthorized)?;
    if session_was_revoked(claims.iat, claims.auth_time, user.session_invalid_before) {
        return Err(DalError::Unauthorized);
    }
    Ok((user, claims))
}

/// Cognito timestamps have whole-second resolution. Revoke the entire cutoff
/// second, including refreshed tokens from an earlier authentication epoch.
pub(crate) fn session_was_revoked(
    issued_at_seconds: i64,
    authenticated_at_seconds: Option<i64>,
    invalid_before: Option<chrono::DateTime<chrono::Utc>>,
) -> bool {
    invalid_before.is_some_and(|cutoff| {
        authenticated_at_seconds
            .is_none_or(|epoch| epoch.min(issued_at_seconds) <= cutoff.timestamp())
    })
}

/// Optional authenticated user — returns `None` for anonymous requests.
pub struct MaybeAuthUser(pub Option<User>);

impl FromRequestParts<AppState> for MaybeAuthUser {
    type Rejection = DalError;

    async fn from_request_parts(
        parts: &mut Parts,
        state: &AppState,
    ) -> Result<Self, Self::Rejection> {
        match AuthUser::from_request_parts(parts, state).await {
            Ok(AuthUser(u)) => Ok(MaybeAuthUser(Some(u))),
            Err(_) => Ok(MaybeAuthUser(None)),
        }
    }
}

fn extract_bearer(headers: &HeaderMap) -> Option<&str> {
    let auth = headers
        .get(axum::http::header::AUTHORIZATION)?
        .to_str()
        .ok()?;
    auth.strip_prefix("Bearer ")
}

fn extract_auth_token(headers: &HeaderMap) -> Option<String> {
    if let Some(token) = extract_bearer(headers) {
        return Some(token.to_string());
    }

    extract_cookie(headers, "dal_access_token")
}

fn extract_cookie(headers: &HeaderMap, name: &str) -> Option<String> {
    let cookies = headers.get(header::COOKIE)?.to_str().ok()?;

    cookies.split(';').find_map(|cookie| {
        let (key, value) = cookie.trim().split_once('=')?;
        if key == name {
            Some(value.to_string())
        } else {
            None
        }
    })
}

/// The user row lock orders grants against global revocation. A raw Cognito JWT
/// cannot stand in for DAL's completed MFA flow, even during a provider downgrade.
pub(crate) async fn bind_mfa_session(
    state: &AppState,
    token: &str,
    refresh: &str,
    is_refresh: bool,
) -> Result<User, DalError> {
    let (user, claims) = validated_identity(state, token).await?;
    let mut tx = dal_db::actor_transaction(&state.db, user.id, None).await?;
    let user = queries::users::lock_session(&mut tx, user.id).await?;
    if !user.email_verified
        || user.cognito_sub != claims.sub
        || session_was_revoked(claims.iat, claims.auth_time, user.session_invalid_before)
    {
        return Err(DalError::Unauthorized);
    }
    let refresh_hash = dal_auth::hash_token(refresh);
    if is_refresh {
        if !queries::mfa_sessions::valid_refresh(&mut tx, user.id, &refresh_hash).await? {
            return Err(DalError::Unauthorized);
        }
    } else {
        queries::mfa_sessions::create(&mut tx, user.id, &refresh_hash).await?;
    }
    let expiry = chrono::DateTime::from_timestamp(claims.exp, 0).ok_or(DalError::Unauthorized)?;
    queries::mfa_sessions::bind_token(&mut tx, &refresh_hash, &dal_auth::hash_token(token), expiry)
        .await?;
    tx.commit().await?;
    Ok(user)
}

#[cfg(test)]
mod session_revocation_tests {
    use super::*;
    use chrono::{TimeZone, Utc};

    #[test]
    fn session_revocation_fails_closed_for_the_cutoff_second() {
        let cutoff = Utc.timestamp_opt(1_700_000_000, 500_000_000).unwrap();
        assert!(session_was_revoked(
            1_700_000_000,
            Some(1_700_000_000),
            Some(cutoff)
        ));
        assert!(!session_was_revoked(
            1_700_000_001,
            Some(1_700_000_001),
            Some(cutoff)
        ));
        assert!(!session_was_revoked(1_700_000_000, None, None));
    }
}
