use axum::{
    extract::{Path, Query, State},
    routing::get,
    Json, Router,
};
use serde::Deserialize;
use serde_json::Value;

use dal_common::{error::DalError, pagination::{Page, PageParams}};
use dal_db::queries;

use crate::state::AppState;

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/packages", get(list_packages))
        .route("/packages/{name}", get(get_package))
}

#[derive(Deserialize)]
struct ListParams {
    #[serde(flatten)]
    page: PageParams,
}

async fn list_packages(
    State(state): State<AppState>,
    Query(params): Query<ListParams>,
) -> Result<Json<Value>, DalError> {
    let (items, total) =
        queries::packages::list(&state.db, params.page.limit(), params.page.offset()).await?;
    let page = Page::new(items, &params.page, total);
    Ok(Json(serde_json::to_value(&page).unwrap_or_default()))
}

async fn get_package(
    State(state): State<AppState>,
    Path(name): Path<String>,
) -> Result<Json<Value>, DalError> {
    let pkg = queries::packages::get_by_name(&state.db, &name)
        .await?
        .ok_or_else(|| DalError::PackageNotFound(name))?;
    Ok(Json(serde_json::to_value(&pkg).unwrap_or_default()))
}
