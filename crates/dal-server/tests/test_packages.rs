mod common;

use axum::http::Method;
use dal_auth::{PUBLISH_NEW_SCOPE, YANK_SCOPE, api_token::hash_token};
use dal_db::{connect, queries};
use serde_json::json;
use uuid::Uuid;

fn raw_token() -> String {
    format!("dal_{}{}", Uuid::new_v4().simple(), Uuid::new_v4().simple())
}

async fn seed_token(db: &sqlx::PgPool, user_id: Uuid, name: &str, scopes: &[String]) -> String {
    queries::users::set_email_verified(db, user_id)
        .await
        .unwrap();
    let token = raw_token();
    let prefix = token[..8].to_string();

    queries::tokens::create(
        db,
        user_id,
        name,
        &hash_token(&token),
        &prefix,
        scopes,
        None,
    )
    .await
    .unwrap();

    token
}

// ── Package listing ───────────────────────────────────────────────────────────

/// GET /packages returns a valid pagination envelope.
#[tokio::test]
async fn list_packages_returns_pagination_envelope() {
    let app = common::TestApp::spawn().await;
    let (status, body) = common::TestApp::unpack(app.get("/packages").await).await;
    assert_eq!(status, 200);
    assert!(body["items"].is_array(), "expected 'items' array");
    assert!(body["total"].is_number(), "expected 'total' number");
    assert!(body["page"].is_number(), "expected 'page' number");
    assert!(body["per_page"].is_number(), "expected 'per_page' number");
    assert!(body["pages"].is_number(), "expected 'pages' number");
}

/// per_page parameter is reflected in the response.
#[tokio::test]
async fn list_packages_respects_per_page_param() {
    let app = common::TestApp::spawn().await;
    let (status, body) =
        common::TestApp::unpack(app.get("/packages?page=1&per_page=5").await).await;
    assert_eq!(status, 200);
    assert_eq!(body["page"], 1);
    assert_eq!(body["per_page"], 5);
    let items = body["items"].as_array().unwrap();
    assert!(items.len() <= 5, "response must not exceed per_page={}", 5);
}

#[tokio::test]
async fn public_catalog_handles_the_maximum_page_without_overflow() {
    let app = common::TestApp::spawn().await;
    let (status, body) =
        common::TestApp::unpack(app.get("/packages?page=4294967295&per_page=100").await).await;
    assert_eq!(status, 200);
    assert_eq!(body["page"], u32::MAX);
    assert_eq!(body["per_page"], 100);
    assert!(body["items"].as_array().unwrap().is_empty());
}

// ── Package lookup ────────────────────────────────────────────────────────────

/// GET /packages/:name for a non-existent name → 404.
#[tokio::test]
async fn get_nonexistent_package_returns_404() {
    let app = common::TestApp::spawn().await;
    let (status, _) = common::TestApp::unpack(
        app.get("/packages/this-package-absolutely-does-not-exist-xyz999")
            .await,
    )
    .await;
    assert_eq!(status, 404);
}

// ── Search ────────────────────────────────────────────────────────────────────

/// GET /search?q= returns a pagination envelope.
#[tokio::test]
async fn search_returns_pagination_envelope() {
    let app = common::TestApp::spawn().await;
    let (status, body) = common::TestApp::unpack(app.get("/search?q=test").await).await;
    assert_eq!(status, 200);
    assert!(body["items"].is_array());
    assert!(body["total"].is_number());
}

/// GET /search without q param — backend should handle gracefully (200 or 422).
#[tokio::test]
async fn search_without_query_does_not_panic() {
    let app = common::TestApp::spawn().await;
    let res = app.get("/search").await;
    // Accept 200 (empty results) or 422 (validation error) — never 500.
    assert_ne!(res.status().as_u16(), 500);
}

// ── Sparse index ──────────────────────────────────────────────────────────────

/// GET /index/:name for non-existent package → 404 (not 500).
#[tokio::test]
async fn index_nonexistent_package_returns_404() {
    let app = common::TestApp::spawn().await;
    let (status, _) = common::TestApp::unpack(app.get("/index/totally-fake-pkg-xyz").await).await;
    assert_eq!(status, 404);
}

#[tokio::test]
async fn yank_requires_yank_scope_for_api_tokens() {
    let app = common::TestApp::spawn().await;
    dotenvy::from_filename(".env.test").ok();
    let database_url = std::env::var("TEST_DATABASE_URL")
        .unwrap_or_else(|_| "postgres://dal_test:test@localhost:5433/dal_test".to_string());
    let db = connect(&database_url).await.unwrap();

    let user_id = Uuid::new_v4();
    let package_id = Uuid::new_v4();
    let version_id = Uuid::new_v4();
    let package_name = format!("scope-yank-{}", Uuid::new_v4().simple());
    let username = format!("scope-yank-user-{}", Uuid::new_v4().simple());

    queries::users::create(
        &db,
        user_id,
        &username,
        &format!("{username}@example.test"),
        &Uuid::new_v4().to_string(),
        None,
    )
    .await
    .unwrap();
    queries::packages::create(
        &db,
        queries::packages::NewPackage {
            id: package_id,
            name: &package_name,
            description: Some("scope yank package"),
            repository: None,
            homepage: None,
            license: None,
            keywords: &[],
            categories: &[],
        },
    )
    .await
    .unwrap();
    queries::packages::add_owner(&db, package_id, user_id, "owner", None)
        .await
        .unwrap();
    queries::versions::create(
        &db,
        queries::versions::NewPackageVersion {
            id: version_id,
            package_id,
            version: "1.0.0",
            checksum: "deadbeef",
            size_bytes: 123,
            s3_key: "packages/test/1.0.0.tar.gz",
            readme: Some("# test"),
            manifest: json!({ "package": { "name": package_name, "version": "1.0.0" } }),
            published_by: user_id,
        },
    )
    .await
    .unwrap();

    let limited = seed_token(
        &db,
        user_id,
        "limited-yank-token",
        &[PUBLISH_NEW_SCOPE.to_string()],
    )
    .await;
    let allowed = seed_token(&db, user_id, "yank-token", &[YANK_SCOPE.to_string()]).await;

    let forbidden = app
        .request_json(
            Method::PUT,
            &format!("/packages/{package_name}/versions/1.0.0/yank"),
            json!({ "reason": "test" }),
            Some(&limited),
        )
        .await;
    assert_eq!(forbidden.status().as_u16(), 403);

    let allowed_res = app
        .request_json(
            Method::PUT,
            &format!("/packages/{package_name}/versions/1.0.0/yank"),
            json!({ "reason": "test" }),
            Some(&allowed),
        )
        .await;
    let (status, body) = common::TestApp::unpack(allowed_res).await;

    assert_eq!(status.as_u16(), 200);
    assert_eq!(body["message"], "yanked");
}

#[tokio::test]
async fn concurrent_publication_preserves_ownership_version_and_uploaded_object() {
    use dal_auth::PUBLISH_UPDATE_SCOPE;
    use flate2::{Compression, write::GzEncoder};
    use std::io::Write;
    let app = common::TestApp::spawn().await;
    let database_url = std::env::var("TEST_DATABASE_URL")
        .unwrap_or_else(|_| "postgres://dal_test:test@localhost:5433/dal_test".to_string());
    let db = connect(&database_url).await.unwrap();
    let id = Uuid::new_v4();
    let name = format!("atomic-publish-{}", id.simple());
    queries::users::create(
        &db,
        id,
        &name,
        &format!("{name}@example.test"),
        &id.to_string(),
        None,
    )
    .await
    .unwrap();
    let token = seed_token(
        &db,
        id,
        "atomic publish",
        &[
            PUBLISH_NEW_SCOPE.to_string(),
            PUBLISH_UPDATE_SCOPE.to_string(),
        ],
    )
    .await;
    for version in ["0.1.0", "0.2.0"] {
        let mut tar = tar::Builder::new(Vec::new());
        let manifest = format!("[package]\nname = \"{name}\"\nversion = \"{version}\"\n");
        for (path, bytes) in [
            ("dal.toml", manifest.as_bytes()),
            ("src/init.fdn", b"action main {}".as_slice()),
        ] {
            let mut header = tar::Header::new_gnu();
            header.set_mode(0o644);
            header.set_size(bytes.len() as u64);
            header.set_cksum();
            tar.append_data(&mut header, format!("{name}-{version}/{path}"), bytes)
                .unwrap();
        }
        let mut gzip = GzEncoder::new(Vec::new(), Compression::default());
        gzip.write_all(&tar.into_inner().unwrap()).unwrap();
        let bytes = gzip.finish().unwrap();
        let (first, second) = tokio::join!(
            app.publish_archive(&name, &token, &bytes),
            app.publish_archive(&name, &token, &bytes)
        );
        let mut codes = [first.status().as_u16(), second.status().as_u16()];
        codes.sort();
        assert_eq!(
            codes,
            [201, 409],
            "one publication must win and the duplicate must conflict"
        );
        let response = app
            .get(&format!("/packages/{name}/versions/{version}/download"))
            .await;
        assert_eq!(response.status(), 307);
        let location = response.headers()["location"].to_str().unwrap();
        let url = url::Url::parse(location).unwrap();
        assert!(matches!(url.host_str(), Some("localhost" | "127.0.0.1")));
        let downloaded = reqwest::get(location).await.unwrap();
        assert!(
            downloaded.status().is_success(),
            "duplicate rollback must not delete the winning archive"
        );
        assert_eq!(downloaded.bytes().await.unwrap().as_ref(), bytes.as_slice());
        let yank_token = seed_token(
            &db,
            id,
            "yank lock test",
            &[dal_auth::YANK_SCOPE.to_string()],
        )
        .await;
        let yanked = app
            .request_json(
                Method::PUT,
                &format!("/packages/{name}/versions/{version}/yank"),
                json!({}),
                Some(&yank_token),
            )
            .await;
        assert_eq!(yanked.status(), 200);
        // Immutable archive access remains available to existing checksum locks.
        assert_eq!(
            app.get(&format!("/packages/{name}/versions/{version}/download"))
                .await
                .status(),
            307
        );
        let unyanked = app
            .request_json(
                Method::PUT,
                &format!("/packages/{name}/versions/{version}/unyank"),
                json!({}),
                Some(&yank_token),
            )
            .await;
        assert_eq!(unyanked.status(), 200);

        let package = queries::packages::get_by_name(&db, &name)
            .await
            .unwrap()
            .unwrap();
        assert!(
            queries::packages::is_owner(&db, package.id, id)
                .await
                .unwrap()
        );
        assert_eq!(
            queries::versions::list_for_package(&db, package.id)
                .await
                .unwrap()
                .len(),
            if version == "0.1.0" { 1 } else { 2 }
        );
    }
}

#[tokio::test]
async fn failed_publication_rolls_back_database_and_cleans_only_its_uploaded_object() {
    use axum::{Router, http::StatusCode};
    use flate2::{Compression, write::GzEncoder};
    use std::{
        io::Write,
        sync::{Arc, Mutex},
    };
    for storage_fails in [true, false] {
        let calls = Arc::new(Mutex::new(Vec::new()));
        let observed = calls.clone();
        let router = Router::new().fallback(move |request: axum::extract::Request| {
            let observed = observed.clone();
            async move {
                observed
                    .lock()
                    .unwrap()
                    .push((request.method().clone(), request.uri().path().to_owned()));
                axum::body::to_bytes(request.into_body(), 1024 * 1024)
                    .await
                    .unwrap();
                if storage_fails {
                    StatusCode::FORBIDDEN
                } else {
                    StatusCode::OK
                }
            }
        });
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let endpoint = format!("http://{}", listener.local_addr().unwrap());
        let service = tokio::spawn(async move { axum::serve(listener, router).await.unwrap() });
        let mut config = common::test_config();
        config.storage_endpoint_url = Some(endpoint);
        let app = common::TestApp::with_config(config).await;
        let db = connect(&common::test_config().database_url).await.unwrap();
        let id = Uuid::new_v4();
        let name = format!("failed-publish-{}", id.simple());
        queries::users::create(
            &db,
            id,
            &name,
            &format!("{name}@example.test"),
            &id.to_string(),
            None,
        )
        .await
        .unwrap();
        let token = seed_token(&db, id, "failure injection", &[PUBLISH_NEW_SCOPE.into()]).await;
        let version = format!("0.1.0-audit{}", id.simple());
        let constraint = format!("audit_publish_failure_{}", id.simple());
        if !storage_fails {
            sqlx::query(&format!("ALTER TABLE package_versions ADD CONSTRAINT {constraint} CHECK (version <> '{version}')")).execute(&db).await.unwrap();
        }
        let mut tar = tar::Builder::new(Vec::new());
        let manifest = format!("[package]\nname=\"{name}\"\nversion=\"{version}\"\n");
        for (path, bytes) in [
            ("dal.toml", manifest.as_bytes()),
            ("src/init.fdn", b"action main {}".as_slice()),
        ] {
            let mut header = tar::Header::new_gnu();
            header.set_mode(0o644);
            header.set_size(bytes.len() as u64);
            header.set_cksum();
            tar.append_data(&mut header, format!("{name}-{version}/{path}"), bytes)
                .unwrap();
        }
        let mut gzip = GzEncoder::new(Vec::new(), Compression::default());
        gzip.write_all(&tar.into_inner().unwrap()).unwrap();
        let response = app
            .publish_archive(&name, &token, &gzip.finish().unwrap())
            .await;
        if !storage_fails {
            sqlx::query(&format!(
                "ALTER TABLE package_versions DROP CONSTRAINT {constraint}"
            ))
            .execute(&db)
            .await
            .unwrap();
        }
        let (status, body) = common::TestApp::unpack(response).await;
        assert_eq!(status, 500);
        assert_eq!(body["error"]["message"], "internal server error");
        assert!(
            queries::packages::get_by_name(&db, &name)
                .await
                .unwrap()
                .is_none()
        );
        let calls = calls.lock().unwrap();
        assert_eq!(calls[0].0, Method::PUT);
        if storage_fails {
            assert!(!calls.iter().any(|c| c.0 == Method::DELETE));
        } else {
            assert_eq!(calls.len(), 2);
            assert_eq!(calls[1].0, Method::DELETE);
            assert_eq!(calls[0].1, calls[1].1);
        }
        service.abort();
    }
}

#[tokio::test]
async fn publish_capacity_backpressures_before_reading_untrusted_archives() {
    let app = common::TestApp::spawn().await;
    assert_eq!(app.publish_slots.available_permits(), 2);
    let db = connect(&common::test_config().database_url).await.unwrap();
    let id = Uuid::new_v4();
    let name = format!("capacity-{}", id.simple());
    queries::users::create(
        &db,
        id,
        &name,
        &format!("{name}@example.test"),
        &id.to_string(),
        None,
    )
    .await
    .unwrap();
    let token = seed_token(&db, id, "capacity", &[PUBLISH_NEW_SCOPE.into()]).await;
    let slots = app.publish_slots.acquire_many(2).await.unwrap();
    let request = app.publish_archive(&name, &token, b"invalid archive");
    tokio::pin!(request);
    assert!(
        tokio::time::timeout(std::time::Duration::from_millis(100), &mut request)
            .await
            .is_err(),
        "request must wait for archive capacity"
    );
    drop(slots);
    let response = tokio::time::timeout(std::time::Duration::from_secs(5), request)
        .await
        .unwrap();
    assert_eq!(response.status(), 422);
    assert_eq!(
        app.publish_slots.available_permits(),
        2,
        "failed archives release capacity"
    );
}

#[tokio::test]
async fn corrupted_persisted_manifest_fails_index_without_disclosing_database_data() {
    let app = common::TestApp::spawn().await;
    let db = connect(&common::test_config().database_url).await.unwrap();
    let id = Uuid::new_v4();
    let package_id = Uuid::new_v4();
    let name = format!("corrupt-index-{}", id.simple());
    queries::users::create(
        &db,
        id,
        &name,
        &format!("{name}@example.test"),
        &id.to_string(),
        None,
    )
    .await
    .unwrap();
    queries::packages::create(
        &db,
        queries::packages::NewPackage {
            id: package_id,
            name: &name,
            description: None,
            repository: None,
            homepage: None,
            license: None,
            keywords: &[],
            categories: &[],
        },
    )
    .await
    .unwrap();
    queries::versions::create(
        &db,
        queries::versions::NewPackageVersion {
            id: Uuid::new_v4(),
            package_id,
            version: "1.0.0",
            checksum: &"a".repeat(64),
            size_bytes: 1,
            s3_key: "unused-local-fixture",
            readme: None,
            manifest: json!({"corrupt": "private-marker"}),
            published_by: id,
        },
    )
    .await
    .unwrap();
    let (status, body) = common::TestApp::unpack(app.get(&format!("/index/{name}")).await).await;
    assert_eq!(status, 500);
    assert!(!body.to_string().contains("private-marker"));
}
