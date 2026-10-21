use axum::{
    Json, Router,
    extract::{Path, State},
    http::StatusCode,
    routing::{delete, get, post},
};
use serde::Deserialize;
use serde_json::{Value, json};
use uuid::Uuid;

use dal_auth::{ApiTokenRaw, api_token::generate_api_token, normalize_scopes};
use dal_common::error::DalError;
use dal_db::queries;

use crate::{extractors::VerifiedSession, state::AppState};

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/tokens", get(list_tokens))
        .route("/tokens", post(create_token))
        .route("/tokens/{id}", delete(delete_token))
}

async fn list_tokens(
    State(state): State<AppState>,
    VerifiedSession(user): VerifiedSession,
) -> Result<Json<Value>, DalError> {
    let tokens = queries::tokens::list_for_user(&state.db, user.id).await?;
    Ok(Json(serde_json::to_value(&tokens).unwrap_or_default()))
}

#[derive(Deserialize)]
struct CreateTokenBody {
    name: String,
    expires_in: Option<u32>, // seconds, None = never expires
    scopes: Option<Vec<String>>,
}

async fn create_token(
    State(state): State<AppState>,
    VerifiedSession(user): VerifiedSession,
    Json(body): Json<CreateTokenBody>,
) -> Result<(StatusCode, Json<Value>), DalError> {
    if body.name.trim().is_empty() || body.name.len() > 64 {
        return Err(DalError::Validation("token name must be 1–64 chars".into()));
    }
    let scopes =
        normalize_scopes(body.scopes.as_deref().unwrap_or(&[])).map_err(DalError::Validation)?;

    let ApiTokenRaw { raw, hash, prefix } = generate_api_token();
    let expires_at = body
        .expires_in
        .map(|secs| chrono::Utc::now() + chrono::Duration::seconds(secs as i64));

    let token_record = queries::tokens::create(
        &state.db,
        user.id,
        body.name.trim(),
        &hash,
        &prefix,
        &scopes,
        expires_at,
    )
    .await?;

    Ok((
        StatusCode::CREATED,
        Json(json!({
            "token": raw,        // shown only once
            "meta": token_record,
        })),
    ))
}

async fn delete_token(
    State(state): State<AppState>,
    VerifiedSession(user): VerifiedSession,
    Path(id): Path<Uuid>,
) -> Result<Json<Value>, DalError> {
    let deleted = queries::tokens::delete(&state.db, id, user.id).await?;
    if !deleted {
        return Err(DalError::Validation("token not found".into()));
    }
    Ok(Json(json!({ "message": "token deleted" })))
}
