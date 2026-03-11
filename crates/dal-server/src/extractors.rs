use axum::{
    extract::FromRequestParts,
    http::{request::Parts, HeaderMap},
};
use dal_auth::Claims;
use dal_common::error::DalError;
use dal_db::{models::User, queries};

use crate::state::AppState;

/// Extractor: authenticated user from JWT (Bearer token in Authorization header)
/// OR API token in `Authorization: Bearer dal_...`.
pub struct AuthUser(pub User);

impl FromRequestParts<AppState> for AuthUser {
    type Rejection = DalError;

    async fn from_request_parts(
        parts: &mut Parts,
        state: &AppState,
    ) -> Result<Self, Self::Rejection> {
        let token = extract_bearer(&parts.headers).ok_or(DalError::Unauthorized)?;

        // Try API token first (fast path — starts with "dal_")
        if token.starts_with("dal_") {
            if !dal_auth::api_token::validate_api_token_format(token) {
                return Err(DalError::InvalidApiToken);
            }
            let hash = dal_auth::hash_token(token);
            let api_tok = queries::tokens::get_by_hash(&state.db, &hash)
                .await?
                .ok_or(DalError::Unauthorized)?;

            // Touch last_used_at (fire-and-forget)
            let db = state.db.clone();
            let id = api_tok.id;
            tokio::spawn(async move {
                let _ = queries::tokens::touch(&db, id).await;
            });

            let user = queries::users::get_by_id(&state.db, api_tok.user_id)
                .await?
                .ok_or(DalError::Unauthorized)?;

            return Ok(AuthUser(user));
        }

        // Otherwise validate as Cognito JWT
        let claims: Claims = state.jwt.validate(token).await?;
        let user = queries::users::get_by_cognito_sub(&state.db, &claims.sub)
            .await?
            .ok_or(DalError::Unauthorized)?;

        Ok(AuthUser(user))
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
    let auth = headers.get(axum::http::header::AUTHORIZATION)?.to_str().ok()?;
    auth.strip_prefix("Bearer ")
}
