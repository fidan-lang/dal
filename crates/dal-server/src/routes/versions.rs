use axum::{
    Json, Router,
    extract::{ConnectInfo, Multipart, Path, State},
    http::{HeaderMap, StatusCode},
    routing::{get, post, put},
};
use serde::Deserialize;
use serde_json::{Value, json};
use tracing::warn;
use uuid::Uuid;

use dal_common::error::DalError;
use dal_db::queries;
use dal_manifest::{Lockfile, Manifest};
use dal_storage::ArchiveInfo;

use crate::{extractors::AuthActor, state::AppState};

const MAX_USER_AGENT_BYTES: usize = 512;

const UPLOAD_CAPACITY_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(30);
const UPLOAD_BODY_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(120);

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/packages/{name}/versions", get(list_versions))
        .route("/packages/{name}/versions/{version}", get(get_version))
        .route(
            "/packages/{name}/versions/{version}/download",
            get(download),
        )
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
    peer: Option<axum::Extension<ConnectInfo<std::net::SocketAddr>>>,
    headers: HeaderMap,
) -> Result<axum::response::Redirect, DalError> {
    let pkg = queries::packages::get_by_name(&state.db, &name)
        .await?
        .ok_or_else(|| DalError::PackageNotFound(name.clone()))?;
    let ver = queries::versions::get(&state.db, pkg.id, &version)
        .await?
        .ok_or_else(|| DalError::VersionNotFound(version.clone(), name.clone()))?;

    // Yank affects resolution, not immutable artifact access for existing locks.
    let url = state.storage.presigned_url(&ver.s3_key, 60).await?;

    let client_ip = peer.map(|axum::Extension(ConnectInfo(address))| {
        crate::middleware::rate_limit::extract_client_ip(
            &headers,
            address.ip(),
            &state.cfg.trusted_proxies,
        )
        .to_string()
    });
    // Without transport peer information, never trust a client-supplied forwarding header.
    let ip_hash = dal_auth::api_token::hash_ip(client_ip.as_deref().unwrap_or("unknown"));
    let ua = headers
        .get(axum::http::header::USER_AGENT)
        .and_then(|value| value.to_str().ok())
        .map(|value| &value[..value.floor_char_boundary(MAX_USER_AGENT_BYTES.min(value.len()))]);
    state
        .best_effort_accounting(queries::versions::account_download(
            &state.db,
            ver.id,
            pkg.id,
            &ip_hash,
            ua,
            crate::state::ACCOUNTING_TIMEOUT,
        ))
        .await;

    Ok(axum::response::Redirect::temporary(&url))
}

/// Publish a new package version.
///
/// Expects `multipart/form-data` with a single field `archive` containing
/// the `.tar.gz` bytes. The archive must contain a canonical
/// `{package}-{version}/...` layout with `dal.toml` at the package root and
/// `src/init.fdn` present as the required package entry point.
async fn publish(
    State(state): State<AppState>,
    actor: AuthActor,
    Path(name): Path<String>,
    mut multipart: Multipart,
) -> Result<(StatusCode, Json<Value>), DalError> {
    // Hold across collection, decompression and storage, not just the HTTP read.
    let _slot = upload_phase(UPLOAD_CAPACITY_TIMEOUT, state.publish_slots.acquire())
        .await?
        .map_err(|_| DalError::Internal(anyhow::anyhow!("publish capacity unavailable")))?;
    // Stream and bound the archive itself, independently of multipart framing.
    let bytes = upload_phase(UPLOAD_BODY_TIMEOUT, async {
        let mut archive_bytes = None;
        while let Some(mut field) = multipart
            .next_field()
            .await
            .map_err(|e| DalError::Validation(format!("multipart error: {e}")))?
        {
            if field.name() != Some("archive") || archive_bytes.is_some() {
                return Err(DalError::Validation(
                    "expected exactly one `archive` multipart field".into(),
                ));
            }
            let mut data = Vec::new();
            while let Some(chunk) = field
                .chunk()
                .await
                .map_err(|e| DalError::Validation(format!("upload read error: {e}")))?
            {
                if (data.len() as u64).saturating_add(chunk.len() as u64)
                    > state.cfg.max_upload_bytes
                {
                    return Err(DalError::ArchiveTooLarge {
                        max_mb: state.cfg.max_upload_bytes / 1024 / 1024,
                    });
                }
                data.extend_from_slice(&chunk);
            }
            archive_bytes = Some(data);
        }
        archive_bytes
            .ok_or_else(|| DalError::Validation("missing `archive` field in multipart".into()))
    })
    .await??;

    // Validate archive (security checks, extract manifest)
    let info = dal_storage::validate_archive(&bytes, state.cfg.max_upload_bytes)?;

    // Parse manifest
    let manifest_bytes = info
        .manifest_bytes
        .as_ref()
        .ok_or_else(|| DalError::ManifestInvalid("dal.toml not found in archive".into()))?;
    let manifest = Manifest::from_toml(manifest_bytes)
        .map_err(|e| DalError::ManifestInvalid(e.to_string()))?;
    validate_publish_contract(&manifest, &info)?;
    if manifest.has_runtime_dependencies() {
        let lock_bytes = dal_storage::extract_file(&bytes, &info.root_dir, "dal.lock")?
            .ok_or_else(|| {
                DalError::ManifestInvalid(
                    "packages with dependencies must include a dal.lock file".into(),
                )
            })?;
        validate_lockfile_bytes(&manifest, &lock_bytes)?;
    }

    // Ensure package name in manifest matches URL
    if !manifest.package.name.eq_ignore_ascii_case(&name) {
        return Err(DalError::Validation(format!(
            "package name in dal.toml (`{}`) does not match URL (`{name}`)",
            manifest.package.name
        )));
    }

    let version_str = manifest.package.version.clone();
    let expected_root_dir = format!("{}-{version_str}", manifest.package.name);
    if info.root_dir != expected_root_dir {
        return Err(DalError::ArchiveInvalid(format!(
            "archive root directory must be `{expected_root_dir}`"
        )));
    }

    let readme = match manifest.package.readme.as_deref() {
        Some(path) => {
            let bytes =
                dal_storage::extract_file(&bytes, &info.root_dir, path)?.ok_or_else(|| {
                    DalError::ManifestInvalid(format!(
                        "declared readme `{path}` not found in archive"
                    ))
                })?;
            Some(String::from_utf8(bytes).map_err(|_| {
                DalError::ManifestInvalid(format!("declared readme `{path}` is not valid UTF-8"))
            })?)
        }
        None => info.readme_bytes.and_then(|b| String::from_utf8(b).ok()),
    };

    let user = &actor.user;
    // One transaction and one connection serialize package creation and publication.
    let mut transaction = state.db.begin().await?;
    sqlx::query("SELECT pg_advisory_xact_lock(hashtextextended($1, 0))")
        .bind(format!("dal-publish:{}", manifest.package.name))
        .execute(&mut *transaction)
        .await?;
    // Get or create the package record
    let pkg = match queries::packages::get_by_name(&mut *transaction, &name).await? {
        Some(p) => {
            actor.require_scope(dal_auth::PUBLISH_UPDATE_SCOPE)?;
            // Ownership check
            if !actor.is_admin()
                && !queries::packages::is_member(&mut *transaction, p.id, user.id).await?
            {
                return Err(DalError::Forbidden);
            }
            p
        }
        None => {
            actor.require_scope(dal_auth::PUBLISH_NEW_SCOPE)?;
            // First publish — create package + set publisher as owner
            let pkg_id = Uuid::new_v4();
            let pkg = queries::packages::create(
                &mut *transaction,
                queries::packages::NewPackage {
                    id: pkg_id,
                    name: &manifest.package.name,
                    description: manifest.package.description.as_deref(),
                    repository: manifest.package.repository.as_deref(),
                    homepage: manifest.package.homepage.as_deref(),
                    license: manifest.package.license.as_deref(),
                    keywords: &manifest.package.keywords,
                    categories: &manifest.package.categories,
                },
            )
            .await?;

            queries::packages::add_owner(&mut *transaction, pkg_id, user.id, "owner", None).await?;
            pkg
        }
    };

    // Check for duplicate version
    if queries::versions::exists(&mut *transaction, pkg.id, &version_str).await? {
        return Err(DalError::VersionAlreadyExists(version_str, name));
    }

    // Upload archive to object storage
    let ver_id = Uuid::new_v4();
    // A failed attempt must never delete another attempt's uploaded object.
    let s3_key = format!(
        "packages/{}/{version_str}/{ver_id}/{}-{version_str}.tar.gz",
        pkg.name, pkg.name
    );
    state
        .storage
        .upload(&s3_key, bytes.to_vec(), "application/gzip")
        .await?;

    // Persist version record
    match queries::versions::create(
        &mut *transaction,
        queries::versions::NewPackageVersion {
            id: ver_id,
            package_id: pkg.id,
            version: &version_str,
            checksum: &info.checksum,
            size_bytes: info.size_bytes,
            s3_key: &s3_key,
            readme: readme.as_deref(),
            manifest: serde_json::to_value(&manifest).unwrap_or_default(),
            published_by: user.id,
        },
    )
    .await
    {
        Ok(_) => {}
        Err(err) => {
            cleanup_failed_publish(&state, &s3_key).await;
            return Err(err);
        }
    };

    // Update package metadata from latest manifest
    if let Err(err) = queries::packages::update_metadata(
        &mut *transaction,
        queries::packages::PackageMetadataUpdate {
            id: pkg.id,
            description: manifest.package.description.as_deref(),
            repository: manifest.package.repository.as_deref(),
            homepage: manifest.package.homepage.as_deref(),
            license: manifest.package.license.as_deref(),
            readme: readme.as_deref(),
            keywords: &manifest.package.keywords,
            categories: &manifest.package.categories,
        },
    )
    .await
    {
        cleanup_failed_publish(&state, &s3_key).await;
        return Err(err);
    }

    // A failed commit can be ambiguous; retain the object rather than break a committed version.
    transaction.commit().await?;

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

    Ok((
        StatusCode::CREATED,
        Json(json!({
            "message": "published",
            "package": pkg.name,
            "version": version_str,
        })),
    ))
}

fn validate_publish_contract(manifest: &Manifest, info: &ArchiveInfo) -> Result<(), DalError> {
    if manifest.has_runtime_dependencies() && !info.files.contains("dal.lock") {
        return Err(DalError::ManifestInvalid(
            "packages with dependencies must include a dal.lock file".into(),
        ));
    }

    if let Some(cli) = &manifest.cli
        && !info.files.contains(&cli.entry)
    {
        return Err(DalError::ManifestInvalid(format!(
            "declared CLI entry `{}` not found in archive",
            cli.entry
        )));
    }

    Ok(())
}

fn validate_lockfile_bytes(manifest: &Manifest, bytes: &[u8]) -> Result<(), DalError> {
    let lock = Lockfile::from_toml(bytes).map_err(|e| DalError::ManifestInvalid(e.to_string()))?;
    lock.validate_against_manifest(manifest)
        .map_err(|e| DalError::ManifestInvalid(e.to_string()))
}

async fn cleanup_failed_publish(state: &AppState, s3_key: &str) {
    if let Err(err) = state.storage.delete(s3_key).await {
        warn!(s3_key, error = %err, "unpublished archive requires reconciliation after failed publish");
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::{BTreeSet, HashMap};

    fn archive_info(files: &[&str]) -> ArchiveInfo {
        ArchiveInfo {
            checksum: "deadbeef".into(),
            size_bytes: 42,
            root_dir: "demo-1.0.0".into(),
            manifest_bytes: None,
            files: files
                .iter()
                .map(|entry| entry.to_string())
                .collect::<BTreeSet<_>>(),
            readme_bytes: None,
            readme_path: None,
        }
    }

    #[test]
    fn publish_contract_requires_lock_for_dependencies() {
        let manifest = Manifest {
            package: dal_manifest::PackageMeta {
                name: "demo".into(),
                version: "1.0.0".into(),
                fidan: None,
                description: None,
                license: None,
                repository: None,
                homepage: None,
                docs: None,
                keywords: vec![],
                categories: vec![],
                readme: None,
                include: vec![],
                exclude: vec![],
            },
            dependencies: HashMap::from([(
                "other-package".into(),
                dal_manifest::DependencySpec::Simple("^1.2".into()),
            )]),
            optional_dependencies: HashMap::new(),
            dev_dependencies: HashMap::new(),
            features: HashMap::new(),
            cli: None,
        };

        let error =
            validate_publish_contract(&manifest, &archive_info(&["dal.toml", "src/init.fdn"]))
                .expect_err("missing dal.lock should fail");
        assert!(error.to_string().contains("must include a dal.lock file"));
    }

    #[test]
    fn publish_contract_requires_lock_for_optional_dependencies() {
        let manifest = Manifest {
            package: dal_manifest::PackageMeta {
                name: "demo".into(),
                version: "1.0.0".into(),
                fidan: None,
                description: None,
                license: None,
                repository: None,
                homepage: None,
                docs: None,
                keywords: vec![],
                categories: vec![],
                readme: None,
                include: vec![],
                exclude: vec![],
            },
            dependencies: HashMap::new(),
            optional_dependencies: HashMap::from([(
                "python-runtime".into(),
                dal_manifest::DependencySpec::Simple("^3".into()),
            )]),
            dev_dependencies: HashMap::new(),
            features: HashMap::from([("pybindings".into(), vec!["dep:python-runtime".into()])]),
            cli: None,
        };

        let error =
            validate_publish_contract(&manifest, &archive_info(&["dal.toml", "src/init.fdn"]))
                .expect_err("missing dal.lock should fail");
        assert!(error.to_string().contains("must include a dal.lock file"));
    }
    #[test]
    fn publish_contract_requires_cli_entry_file() {
        let manifest = Manifest {
            package: dal_manifest::PackageMeta {
                name: "demo".into(),
                version: "1.0.0".into(),
                fidan: None,
                description: None,
                license: None,
                repository: None,
                homepage: None,
                docs: None,
                keywords: vec![],
                categories: vec![],
                readme: None,
                include: vec![],
                exclude: vec![],
            },
            dependencies: HashMap::new(),
            optional_dependencies: HashMap::new(),
            dev_dependencies: HashMap::new(),
            features: HashMap::new(),
            cli: Some(dal_manifest::CliMeta {
                entry: "src/main.fdn".into(),
                name: None,
            }),
        };

        let error =
            validate_publish_contract(&manifest, &archive_info(&["dal.toml", "src/init.fdn"]))
                .expect_err("missing cli entry should fail");
        assert!(
            error
                .to_string()
                .contains("declared CLI entry `src/main.fdn` not found")
        );
    }

    #[test]
    fn lockfile_validation_rejects_mismatched_direct_dependency_version() {
        let manifest = Manifest {
            package: dal_manifest::PackageMeta {
                name: "demo".into(),
                version: "1.0.0".into(),
                fidan: None,
                description: None,
                license: None,
                repository: None,
                homepage: None,
                docs: None,
                keywords: vec![],
                categories: vec![],
                readme: None,
                include: vec![],
                exclude: vec![],
            },
            dependencies: HashMap::from([(
                "other-package".into(),
                dal_manifest::DependencySpec::Simple("^2.0".into()),
            )]),
            optional_dependencies: HashMap::new(),
            dev_dependencies: HashMap::new(),
            features: HashMap::new(),
            cli: None,
        };

        let error = validate_lockfile_bytes(
            &manifest,
            br#"
schema_version = 1

[[packages]]
name = "other-package"
module = "other_package"
version = "1.2.3"
"#,
        )
        .expect_err("mismatched direct dependency should fail");
        assert!(
            error
                .to_string()
                .contains("requires `^2.0` but lock pins `1.2.3`")
        );
    }
}

// ── Yank / Unyank ─────────────────────────────────────────────────────────────

#[derive(Deserialize)]
struct YankBody {
    reason: Option<String>,
}

async fn yank(
    State(state): State<AppState>,
    actor: AuthActor,
    Path((name, version)): Path<(String, String)>,
    Json(body): Json<YankBody>,
) -> Result<Json<Value>, DalError> {
    actor.require_scope(dal_auth::YANK_SCOPE)?;

    let pkg = queries::packages::get_by_name(&state.db, &name)
        .await?
        .ok_or_else(|| DalError::PackageNotFound(name.clone()))?;

    if !actor.is_admin() && !queries::packages::is_member(&state.db, pkg.id, actor.user.id).await? {
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
    actor: AuthActor,
    Path((name, version)): Path<(String, String)>,
) -> Result<Json<Value>, DalError> {
    actor.require_scope(dal_auth::YANK_SCOPE)?;

    let pkg = queries::packages::get_by_name(&state.db, &name)
        .await?
        .ok_or_else(|| DalError::PackageNotFound(name.clone()))?;

    if !actor.is_admin() && !queries::packages::is_member(&state.db, pkg.id, actor.user.id).await? {
        return Err(DalError::Forbidden);
    }

    let ok = queries::versions::unyank(&state.db, pkg.id, &version).await?;
    if !ok {
        return Err(DalError::VersionNotFound(version, name));
    }

    Ok(Json(json!({ "message": "unyanked" })))
}

// Apply a total wall-clock budget, so dribbling chunks cannot retain archive slots.
// DB/object-store phases keep their existing transaction and SDK timeout semantics.
async fn upload_phase<T>(
    duration: std::time::Duration,
    operation: impl std::future::Future<Output = T>,
) -> Result<T, DalError> {
    tokio::time::timeout(duration, operation)
        .await
        .map_err(|_| DalError::UploadTimeout)
}

#[cfg(test)]
mod upload_timeout_tests {
    use super::*;
    #[tokio::test]
    async fn stalled_upload_phase_times_out_with_public_http_contract() {
        let error = upload_phase(
            std::time::Duration::from_millis(1),
            std::future::pending::<()>(),
        )
        .await
        .unwrap_err();
        assert_eq!(error.status(), StatusCode::REQUEST_TIMEOUT);
        assert_eq!(error.code(), "UPLOAD_TIMEOUT");
        assert_eq!(
            upload_phase(std::time::Duration::from_secs(1), std::future::ready(42))
                .await
                .unwrap(),
            42
        );
    }
}
