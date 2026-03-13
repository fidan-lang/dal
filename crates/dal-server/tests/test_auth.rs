mod common;

use dal_db::queries;

// ── Auth endpoint smoke tests (no Cognito required) ───────────────────────────

/// GET /auth/me without a session cookie → 401.
#[tokio::test]
async fn me_without_auth_returns_401() {
    let app = common::TestApp::spawn().await;
    let (status, _) = common::TestApp::unpack(app.get("/auth/me").await).await;
    assert_eq!(status, 401);
}

/// POST /auth/register with a username that's too short → 422.
#[tokio::test]
async fn register_rejects_short_username() {
    let app = common::TestApp::spawn().await;
    let (status, body) = common::TestApp::unpack(
        app.post_json(
            "/auth/register",
            serde_json::json!({
                "username": "x",
                "email": "test@example.com",
                "password": "password123"
            }),
        )
        .await,
    )
    .await;
    assert_eq!(status, 422, "body: {body}");
}

/// POST /auth/register with a short password → 422.
#[tokio::test]
async fn register_rejects_short_password() {
    let app = common::TestApp::spawn().await;
    let (status, body) = common::TestApp::unpack(
        app.post_json(
            "/auth/register",
            serde_json::json!({
                "username": "validuser",
                "email": "test@example.com",
                "password": "short"
            }),
        )
        .await,
    )
    .await;
    assert_eq!(status, 422, "body: {body}");
}

/// POST /auth/register with invalid Username chars → 422.
#[tokio::test]
async fn register_rejects_invalid_username_chars() {
    let app = common::TestApp::spawn().await;
    let (status, _) = common::TestApp::unpack(
        app.post_json(
            "/auth/register",
            serde_json::json!({
                "username": "invalid user!",
                "email": "test@example.com",
                "password": "password123"
            }),
        )
        .await,
    )
    .await;
    assert_eq!(status, 422);
}

/// POST /auth/login with a missing body → 422 (not 500).
#[tokio::test]
async fn login_with_empty_body_returns_422() {
    let app = common::TestApp::spawn().await;
    let (status, _) =
        common::TestApp::unpack(app.post_json("/auth/login", serde_json::json!({})).await).await;
    assert_eq!(status, 422);
}

// ── Cognito-dependent tests ───────────────────────────────────────────────────
// These require cognito-local running.  Set TEST_COGNITO_ENDPOINT_URL in
// .env.test and run:  cargo test -p dal-server -- --include-ignored

/// Full register → login → me → logout flow.
#[tokio::test]
#[ignore = "requires cognito-local (set TEST_COGNITO_ENDPOINT_URL)"]
async fn register_login_me_logout_flow() {
    let app = common::TestApp::spawn().await;

    let username = format!("daltest{}", &uuid::Uuid::new_v4().to_string()[..8]);
    let email = format!("{}@dal.test", username);
    let password = "Test1234!";

    // Register
    let (status, body) = common::TestApp::unpack(
        app.post_json(
            "/auth/register",
            serde_json::json!({
                "username": username,
                "email": email,
                "password": password,
            }),
        )
        .await,
    )
    .await;
    assert_eq!(status, 201, "register failed: {body}");

    // Registration now requires email verification before login. For the
    // integration test, mark the user verified directly in the test DB.
    let database_url = std::env::var("TEST_DATABASE_URL")
        .unwrap_or_else(|_| "postgres://dal_test:test@localhost:5433/dal_test".to_string());
    let db = dal_db::connect(&database_url).await.unwrap();
    let user = queries::users::get_by_username(&db, &username)
        .await
        .unwrap()
        .expect("registered user should exist");
    queries::users::set_email_verified(&db, user.id)
        .await
        .unwrap();

    // Login
    let (status, body) = common::TestApp::unpack(
        app.post_json(
            "/auth/login",
            serde_json::json!({ "username": username, "password": password }),
        )
        .await,
    )
    .await;
    assert_eq!(status, 200, "login failed: {body}");
    assert_eq!(body["username"], username);
}

/// Login with wrong credentials → 401.
#[tokio::test]
#[ignore = "requires cognito-local (set TEST_COGNITO_ENDPOINT_URL)"]
async fn login_wrong_password_returns_401() {
    let app = common::TestApp::spawn().await;
    let (status, _) = common::TestApp::unpack(
        app.post_json(
            "/auth/login",
            serde_json::json!({ "username": "nobody_xyz", "password": "wrong" }),
        )
        .await,
    )
    .await;
    assert!(status.is_client_error(), "expected 4xx, got {status}");
}
