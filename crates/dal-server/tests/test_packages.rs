mod common;

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
