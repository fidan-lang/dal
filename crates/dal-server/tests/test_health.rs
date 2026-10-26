mod common;

/// GET /health → 200 {"status":"ok"}
#[tokio::test]
async fn health_returns_ok() {
    let app = common::TestApp::spawn().await;
    let (status, body) = common::TestApp::unpack(app.get("/health").await).await;
    assert_eq!(status, 200);
    assert_eq!(body["status"], "ok");
}

/// GET /readyz → 200 {"status":"ready","db":"ok"}
#[tokio::test]
async fn readyz_returns_ready_when_db_up() {
    let app = common::TestApp::spawn().await;
    let (status, body) = common::TestApp::unpack(app.get("/readyz").await).await;
    assert_eq!(status, 200);
    assert_eq!(body["db"], "ok");
    assert_eq!(body["status"], "ready");
}

/// Any unknown path → the router should return 404 (Axum default).
#[tokio::test]
async fn unknown_route_returns_404() {
    let app = common::TestApp::spawn().await;
    let res = app.get("/this/path/does/not/exist").await;
    assert_eq!(res.status(), 404);
}

#[tokio::test]
async fn readyz_returns_unavailable_when_its_database_pool_is_closed() {
    use axum::{body::Body, http::Request};
    use dal_server::{app::build_router, state::AppState};
    use tower::ServiceExt;
    let state = AppState::build(&common::test_config()).await.unwrap();
    state.db.close().await;
    let response = build_router(state)
        .oneshot(
            Request::builder()
                .uri("/readyz")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    let (status, body) = common::TestApp::unpack(response).await;
    assert_eq!(status, 503);
    assert_eq!(body["db"], "error");
}
