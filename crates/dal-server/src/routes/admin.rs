use axum::{
    Json, Router,
    extract::{Path, State},
    routing::delete,
};
use serde_json::json;
use tracing::warn;

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

    let deleted = queries::packages::delete(&state.db, pkg.id).await?;
    if !deleted {
        return Err(DalError::PackageNotFound(name));
    }

    let mut failed_cleanup_keys = Vec::new();
    for version in &versions {
        if let Err(err) = state.storage.delete(&version.s3_key).await {
            warn!(
                package = %pkg.name,
                s3_key = %version.s3_key,
                error = %err,
                "failed to delete package archive after database delete"
            );
            failed_cleanup_keys.push(version.s3_key.clone());
        }
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

    Ok(Json(json!({
        "message": "package deleted",
        "storage_cleanup_failed": !failed_cleanup_keys.is_empty(),
        "failed_cleanup_keys": failed_cleanup_keys,
    })))
}
