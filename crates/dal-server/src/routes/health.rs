use axum::{routing::get, Json, Router};
use serde_json::json;

use crate::state::AppState;

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/health", get(health))
        .route("/readyz", get(readyz))
}

async fn health() -> Json<serde_json::Value> {
    Json(json!({ "status": "ok" }))
}

async fn readyz(axum::extract::State(state): axum::extract::State<AppState>) -> Json<serde_json::Value> {
    match sqlx::query("SELECT 1").execute(&state.db).await {
        Ok(_) => Json(json!({ "status": "ready", "db": "ok" })),
        Err(_) => Json(json!({ "status": "degraded", "db": "error" })),
    }
}
