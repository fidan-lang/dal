use axum::http::{StatusCode, header};
use axum::response::{IntoResponse, Response};
use axum::{
    Router,
    extract::{Path, State},
    routing::get,
};

use dal_common::error::DalError;
use dal_db::queries;
use dal_index::{IndexEntry, build_index_ndjson, entry_from_version};

use crate::state::AppState;

pub fn router() -> Router<AppState> {
    Router::new().route("/index/{name}", get(index_file))
}

/// Serve the sparse index NDJSON for a package.
async fn index_file(
    State(state): State<AppState>,
    Path(name): Path<String>,
) -> Result<Response, DalError> {
    let pkg = queries::packages::get_by_name(&state.db, &name)
        .await?
        .ok_or_else(|| DalError::PackageNotFound(name.clone()))?;

    let versions = queries::versions::list_for_package(&state.db, pkg.id).await?;

    let mut entries: Vec<IndexEntry> = Vec::new();
    for ver in &versions {
        if let Ok(manifest) =
            serde_json::from_value::<dal_manifest::Manifest>(ver.manifest.clone().0)
        {
            let entry = entry_from_version(
                &pkg.name,
                &ver.version,
                &ver.checksum,
                ver.yanked,
                pkg.license.as_deref(),
                &manifest,
            );
            entries.push(entry);
        }
    }

    let body = build_index_ndjson(&entries);

    Ok((
        StatusCode::OK,
        [(header::CONTENT_TYPE, "application/x-ndjson")],
        body,
    )
        .into_response())
}
