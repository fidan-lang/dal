use axum::{
    extract::{Query, State},
    routing::get,
    Json, Router,
};
use serde::Deserialize;
use serde_json::Value;

use dal_common::{error::DalError, pagination::{Page, PageParams}};
use dal_db::queries;

use crate::state::AppState;

pub fn router() -> Router<AppState> {
    Router::new().route("/search", get(search))
}

#[derive(Deserialize)]
struct SearchParams {
    q: String,
    #[serde(flatten)]
    page: PageParams,
}

async fn search(
    State(state): State<AppState>,
    Query(params): Query<SearchParams>,
) -> Result<Json<Value>, DalError> {
    let q = params.q.trim().to_string();
    if q.is_empty() {
        return Err(DalError::Validation("search query `q` must not be empty".into()));
    }
    if q.len() > 128 {
        return Err(DalError::Validation("search query must be ≤ 128 chars".into()));
    }

    let (items, total) = queries::packages::search(
        &state.db,
        &q,
        params.page.limit(),
        params.page.offset(),
    )
    .await?;

    let page = Page::new(items, &params.page, total);
    Ok(Json(serde_json::to_value(&page).unwrap_or_default()))
}
