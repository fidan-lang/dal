use axum::{
    Json, Router,
    extract::{Query, State},
    routing::get,
};
use serde::Deserialize;
use serde_json::Value;

use dal_common::{
    error::DalError,
    pagination::{Page, PageParams},
};
use dal_db::queries;

use crate::state::AppState;

pub fn router() -> Router<AppState> {
    Router::new().route("/search", get(search))
}

#[derive(Deserialize)]
struct SearchParams {
    #[serde(default)]
    q: String,
    #[serde(default)]
    category: Option<String>,
    #[serde(flatten)]
    page: PageParams,
}

async fn search(
    State(state): State<AppState>,
    Query(params): Query<SearchParams>,
) -> Result<Json<Value>, DalError> {
    let q = params.q.trim();
    let category = params
        .category
        .as_deref()
        .map(str::trim)
        .filter(|c| !c.is_empty());
    if q.is_empty() && category.is_none() {
        return Err(DalError::Validation(
            "provide a search query or category".into(),
        ));
    }
    if q.len() > 128 {
        return Err(DalError::Validation(
            "search query must be ≤ 128 chars".into(),
        ));
    }
    if category.is_some_and(|value| value.len() > 64) {
        return Err(DalError::Validation(
            "category filter must be ≤ 64 chars".into(),
        ));
    }

    let (items, total) = queries::packages::search(
        &state.db,
        q,
        category,
        params.page.limit(),
        params.page.offset(),
    )
    .await?;

    let page = Page::new(items, &params.page, total);
    Ok(Json(serde_json::to_value(&page).unwrap_or_default()))
}
