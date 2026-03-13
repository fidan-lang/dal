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
        package_id,
        &package_name,
        Some("scope yank package"),
        None,
        None,
        None,
        &[],
        &[],
    )
    .await
    .unwrap();
    queries::packages::add_owner(&db, package_id, user_id, "owner", None)
        .await
        .unwrap();
    queries::versions::create(
        &db,
        version_id,
        package_id,
        "1.0.0",
        "deadbeef",
        123,
        "packages/test/1.0.0.tar.gz",
        Some("# test"),
        json!({ "package": { "name": package_name, "version": "1.0.0" } }),
        user_id,
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
