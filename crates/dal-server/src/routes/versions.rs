use axum::{
    body::Bytes,
    extract::{Multipart, Path, State},
    http::{HeaderMap, StatusCode},
    routing::{get, post, put},
    Json, Router,
};
use serde::Deserialize;
use serde_json::{json, Value};
use uuid::Uuid;

use dal_common::error::DalError;
use dal_db::queries;
use dal_manifest::Manifest;
use dal_storage::StorageClient;

use crate::{extractors::AuthUser, state::AppState};

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/packages/{name}/versions", get(list_versions))
        .route("/packages/{name}/versions/{version}", get(get_version))
        .route("/packages/{name}/versions/{version}/download", get(download))
        .route("/packages/{name}/publish", post(publish))
        .route("/packages/{name}/versions/{version}/yank", put(yank))
        .route("/packages/{name}/versions/{version}/unyank", put(unyank))
}

async fn list_versions(
    State(state): State<AppState>,
    Path(name): Path<String>,
) -> Result<Json<Value>, DalError> {
    let pkg = queries::packages::get_by_name(&state.db, &name)
        .await?
        .ok_or_else(|| DalError::PackageNotFound(name))?;
    let versions = queries::versions::list_for_package(&state.db, pkg.id).await?;
    Ok(Json(serde_json::to_value(&versions).unwrap_or_default()))
}

async fn get_version(
    State(state): State<AppState>,
    Path((name, version)): Path<(String, String)>,
) -> Result<Json<Value>, DalError> {
    let pkg = queries::packages::get_by_name(&state.db, &name)
        .await?
        .ok_or_else(|| DalError::PackageNotFound(name.clone()))?;
    let ver = queries::versions::get(&state.db, pkg.id, &version)
        .await?
        .ok_or_else(|| DalError::VersionNotFound(version, name))?;
    Ok(Json(serde_json::to_value(&ver).unwrap_or_default()))
}

async fn download(
    State(state): State<AppState>,
    Path((name, version)): Path<(String, String)>,
    headers: HeaderMap,
) -> Result<axum::response::Redirect, DalError> {
    let pkg = queries::packages::get_by_name(&state.db, &name)
        .await?
        .ok_or_else(|| DalError::PackageNotFound(name.clone()))?;
    let ver = queries::versions::get(&state.db, pkg.id, &version)
        .await?
        .ok_or_else(|| DalError::VersionNotFound(version.clone(), name.clone()))?;

    if ver.yanked {
        return Err(DalError::Validation(format!(
            "version `{version}` of `{name}` has been yanked"
        )));
    }

    let url = state.storage.presigned_url(&ver.s3_key, 60).await?;

    // Record download asynchronously
    let db = state.db.clone();
    let client_ip = headers
        .get("x-forwarded-for")
        .and_then(|value| value.to_str().ok())
        .and_then(|value| value.split(',').next())
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .unwrap_or("unknown");
    let ip_hash = dal_auth::api_token::hash_ip(client_ip);
    let ver_id = ver.id;
    let pkg_id = pkg.id;
    let ua = headers
        .get(axum::http::header::USER_AGENT)
        .and_then(|value| value.to_str().ok())
        .map(ToOwned::to_owned);
    tokio::spawn(async move {
        let _ = queries::versions::increment_downloads(&db, ver_id).await;
        let _ = queries::packages::increment_downloads(&db, pkg_id).await;
        let _ = queries::versions::record_download(&db, ver_id, &ip_hash, ua.as_deref()).await;
    });

    Ok(axum::response::Redirect::temporary(&url))
}

/// Publish a new package version.
///
/// Expects `multipart/form-data` with a single field `archive` containing
/// the `.tar.gz` bytes. The archive must contain `dal.toml` at its root.
async fn publish(
    State(state): State<AppState>,
    AuthUser(user): AuthUser,
    Path(name): Path<String>,
    mut multipart: Multipart,
) -> Result<(StatusCode, Json<Value>), DalError> {
    // Extract archive bytes from multipart
    let mut archive_bytes: Option<Bytes> = None;
    while let Some(field) = multipart.next_field().await.map_err(|e| {
        DalError::Validation(format!("multipart error: {e}"))
    })? {
        if field.name() == Some("archive") {
            let data = field.bytes().await.map_err(|e| {
                DalError::Validation(format!("upload read error: {e}"))
            })?;
            archive_bytes = Some(data);
            break;
        }
    }

    let bytes = archive_bytes
        .ok_or_else(|| DalError::Validation("missing `archive` field in multipart".into()))?;

    // Validate archive (security checks, extract manifest)
    let info = dal_storage::validate_archive(&bytes, state.cfg.max_upload_bytes)?;

    // Parse manifest
    let manifest_bytes = info
        .manifest_bytes
        .ok_or_else(|| DalError::ManifestInvalid("dal.toml not found in archive".into()))?;
    let manifest = Manifest::from_toml(&manifest_bytes)
        .map_err(|e| DalError::ManifestInvalid(e.to_string()))?;

    // Ensure package name in manifest matches URL
    if !manifest.package.name.eq_ignore_ascii_case(&name) {
        return Err(DalError::Validation(format!(
            "package name in dal.toml (`{}`) does not match URL (`{name}`)",
            manifest.package.name
        )));
    }

    let version_str = manifest.package.version.clone();

    // Get or create the package record
    let pkg = match queries::packages::get_by_name(&state.db, &name).await? {
        Some(p) => {
            // Ownership check
            if !queries::packages::is_member(&state.db, p.id, user.id).await? {
                return Err(DalError::Forbidden);
            }
            p
        }
        None => {
            // First publish — create package + set publisher as owner
            let pkg_id = Uuid::new_v4();
            let pkg = queries::packages::create(
                &state.db,
                pkg_id,
                &manifest.package.name,
                manifest.package.description.as_deref(),
                manifest.package.repository.as_deref(),
                manifest.package.homepage.as_deref(),
                manifest.package.license.as_deref(),
                &manifest.package.keywords,
                &manifest.package.categories,
            )
            .await?;

            queries::packages::add_owner(&state.db, pkg_id, user.id, "owner", None).await?;
            pkg
        }
    };

    // Check for duplicate version
    if queries::versions::exists(&state.db, pkg.id, &version_str).await? {
        return Err(DalError::VersionAlreadyExists(version_str, name));
    }

    // Upload archive to S3
    let s3_key = StorageClient::object_key(&pkg.name, &version_str);
    state
        .storage
        .upload(&s3_key, bytes.to_vec(), "application/gzip")
        .await?;

    // Persist version record
    let ver_id = Uuid::new_v4();
    let readme = info
        .readme_bytes
        .and_then(|b| String::from_utf8(b).ok());

    queries::versions::create(
        &state.db,
        ver_id,
        pkg.id,
        &version_str,
        &info.checksum,
        info.size_bytes,
        &s3_key,
        readme.as_deref(),
        serde_json::to_value(&manifest).unwrap_or_default(),
        user.id,
    )
    .await?;

    // Update package metadata from latest manifest
    queries::packages::update_metadata(
        &state.db,
        pkg.id,
        manifest.package.description.as_deref(),
        manifest.package.repository.as_deref(),
        manifest.package.homepage.as_deref(),
        manifest.package.license.as_deref(),
        readme.as_deref(),
        &manifest.package.keywords,
        &manifest.package.categories,
    )
    .await?;

    // Audit log
    let _ = queries::audit::record(
        &state.db,
        Some(user.id),
        "publish",
        Some(ver_id),
        Some("version"),
        json!({ "package": pkg.name, "version": version_str }),
    )
    .await;

    Ok((StatusCode::CREATED, Json(json!({
        "message": "published",
        "package": pkg.name,
        "version": version_str,
    }))))
}

// ── Yank / Unyank ─────────────────────────────────────────────────────────────

#[derive(Deserialize)]
struct YankBody {
    reason: Option<String>,
}

async fn yank(
    State(state): State<AppState>,
    AuthUser(user): AuthUser,
    Path((name, version)): Path<(String, String)>,
    Json(body): Json<YankBody>,
) -> Result<Json<Value>, DalError> {
    let pkg = queries::packages::get_by_name(&state.db, &name)
        .await?
        .ok_or_else(|| DalError::PackageNotFound(name.clone()))?;

    if !queries::packages::is_member(&state.db, pkg.id, user.id).await? {
        return Err(DalError::Forbidden);
    }

    let ok = queries::versions::yank(&state.db, pkg.id, &version, body.reason.as_deref()).await?;
    if !ok {
        return Err(DalError::VersionNotFound(version, name));
    }

    Ok(Json(json!({ "message": "yanked" })))
}

async fn unyank(
    State(state): State<AppState>,
    AuthUser(user): AuthUser,
    Path((name, version)): Path<(String, String)>,
) -> Result<Json<Value>, DalError> {
    let pkg = queries::packages::get_by_name(&state.db, &name)
        .await?
        .ok_or_else(|| DalError::PackageNotFound(name.clone()))?;

    if !queries::packages::is_member(&state.db, pkg.id, user.id).await? {
        return Err(DalError::Forbidden);
    }

    let ok = queries::versions::unyank(&state.db, pkg.id, &version).await?;
    if !ok {
        return Err(DalError::VersionNotFound(version, name));
    }

    Ok(Json(json!({ "message": "unyanked" })))
}
