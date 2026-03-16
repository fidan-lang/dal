use axum::{
    Json, Router,
    extract::{Path, State},
    routing::delete,
};
use serde_json::json;

use dal_common::error::DalError;
use dal_db::queries;

use crate::{extractors::AuthActor, state::AppState};

pub fn router() -> Router<AppState> {
    Router::new().route("/admin/packages/{name}", delete(delete_package))
}

async fn delete_package(
    State(state): State<AppState>,
    actor: AuthActor,
    Path(name): Path<String>,
) -> Result<Json<serde_json::Value>, DalError> {
    actor.require_admin()?;

    let pkg = queries::packages::get_by_name(&state.db, &name)
        .await?
        .ok_or_else(|| DalError::PackageNotFound(name.clone()))?;
    let versions = queries::versions::list_for_package(&state.db, pkg.id).await?;

    for version in &versions {
        state.storage.delete(&version.s3_key).await?;
    }

    let deleted = queries::packages::delete(&state.db, pkg.id).await?;
    if !deleted {
        return Err(DalError::PackageNotFound(name));
    }

    let _ = queries::audit::record(
        &state.db,
        Some(actor.user.id),
        "admin_delete_package",
        Some(pkg.id),
        Some("package"),
        json!({ "package": pkg.name }),
    )
    .await;

    Ok(Json(json!({ "message": "package deleted" })))
}
