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
pub struct AuthActor {
    pub user: User,
    pub api_token: Option<ApiToken>,
}

impl AuthActor {
    pub fn require_scope(&self, scope: &str) -> Result<(), DalError> {
        if let Some(token) = &self.api_token {
            if !has_scope(&token.scopes, scope) {
                return Err(DalError::Forbidden);
            }
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

            // Touch last_used_at (fire-and-forget)
            let db = state.db.clone();
            let id = api_tok.id;
            tokio::spawn(async move {
                let _ = queries::tokens::touch(&db, id).await;
            });

            let user = queries::users::get_by_id(&state.db, api_tok.user_id).await?;
            let user = user.ok_or(DalError::Unauthorized)?;

            return Ok(AuthActor {
                user,
                api_token: Some(api_tok),
            });
        }

        // Otherwise validate as Cognito JWT
        let claims: Claims = state.jwt.validate(&token).await?;
        let user = queries::users::get_by_cognito_sub(&state.db, &claims.sub)
            .await?
            .ok_or(DalError::Unauthorized)?;

        Ok(AuthActor {
            user,
            api_token: None,
        })
    }
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
