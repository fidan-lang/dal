use axum::{
    extract::{Path, State},
    http::StatusCode,
    routing::{delete, get, post},
    Json, Router,
};
use serde::Deserialize;
use serde_json::{json, Value};
use uuid::Uuid;

use dal_auth::{api_token::generate_api_token, ApiTokenRaw};
use dal_common::error::DalError;
use dal_db::queries;

use crate::{extractors::AuthUser, state::AppState};

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/tokens", get(list_tokens))
        .route("/tokens", post(create_token))
        .route("/tokens/{id}", delete(delete_token))
}

async fn list_tokens(
    State(state): State<AppState>,
    AuthUser(user): AuthUser,
) -> Result<Json<Value>, DalError> {
    let tokens = queries::tokens::list_for_user(&state.db, user.id).await?;
    Ok(Json(serde_json::to_value(&tokens).unwrap_or_default()))
}

#[derive(Deserialize)]
struct CreateTokenBody {
    name:       String,
    expires_in: Option<u32>,  // seconds, None = never expires
}

async fn create_token(
    State(state): State<AppState>,
    AuthUser(user): AuthUser,
    Json(body): Json<CreateTokenBody>,
) -> Result<(StatusCode, Json<Value>), DalError> {
    if body.name.trim().is_empty() || body.name.len() > 64 {
        return Err(DalError::Validation("token name must be 1–64 chars".into()));
    }

    let ApiTokenRaw { raw, hash, prefix } = generate_api_token();
    let expires_at = body.expires_in.map(|secs| {
        chrono::Utc::now() + chrono::Duration::seconds(secs as i64)
    });

    let token_record = queries::tokens::create(
        &state.db,
        user.id,
        body.name.trim(),
        &hash,
        &prefix,
        expires_at,
    )
    .await?;

    Ok((StatusCode::CREATED, Json(json!({
        "token": raw,        // shown only once
        "id":    token_record.id,
        "name":  token_record.name,
        "prefix": token_record.prefix,
    }))))
}

async fn delete_token(
    State(state): State<AppState>,
    AuthUser(user): AuthUser,
    Path(id): Path<Uuid>,
) -> Result<Json<Value>, DalError> {
    let deleted = queries::tokens::delete(&state.db, id, user.id).await?;
    if !deleted {
        return Err(DalError::Validation("token not found".into()));
    }
    Ok(Json(json!({ "message": "token deleted" })))
}
