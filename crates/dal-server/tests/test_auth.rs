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

#[tokio::test]
async fn partial_registration_removes_only_its_new_provider_identity_without_creating_db_user() {
    use axum::{
        Json, Router,
        http::{HeaderMap, StatusCode},
    };
    use serde_json::json;
    use std::sync::{Arc, Mutex};
    for failure in ["password", "subject", "token-store"] {
        let subject = uuid::Uuid::new_v4().to_string();
        let returned_subject = subject.clone();
        let calls = Arc::new(Mutex::new(Vec::new()));
        let seen = calls.clone();
        let router = Router::new().fallback(move |headers: HeaderMap| {
            let seen = seen.clone();
            let subject = returned_subject.clone();
            async move {
                let target = headers.get("x-amz-target").unwrap().to_str().unwrap().rsplit('.').next().unwrap().to_owned();
                seen.lock().unwrap().push(target.clone());
                match target.as_str() {
                    "AdminCreateUser" => (StatusCode::OK, Json(json!({"User":{"Username":"fixture", "Attributes": if failure == "subject" {json!([])} else {json!([{"Name":"sub","Value":subject}])}}}))),
                    "AdminSetUserPassword" if failure == "password" => (StatusCode::BAD_REQUEST, Json(json!({"__type":"InvalidPasswordException","message":"fixture password rejection"}))),
                    "AdminSetUserPassword" | "AdminDeleteUser" => (StatusCode::OK, Json(json!({}))),
                    _ => panic!("unexpected provider operation"),
                }
            }
        });
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let endpoint = format!("http://{}", listener.local_addr().unwrap());
        let service = tokio::spawn(async move { axum::serve(listener, router).await.unwrap() });
        let mut cfg = common::test_config();
        cfg.cognito_endpoint_url = Some(endpoint);
        let app = common::TestApp::with_config(cfg.clone()).await;
        let db = dal_db::connect(&cfg.database_url).await.unwrap();
        let name = format!(
            "rollback-{}",
            &uuid::Uuid::new_v4().simple().to_string()[..12]
        );
        let hook = format!("audit_token_{}", uuid::Uuid::new_v4().simple());
        if failure == "token-store" {
            sqlx::query(&format!("CREATE FUNCTION {hook}() RETURNS trigger LANGUAGE plpgsql AS $$ BEGIN IF EXISTS (SELECT 1 FROM users WHERE id = NEW.user_id AND cognito_sub = '{subject}') THEN RAISE EXCEPTION 'local token failure'; END IF; RETURN NEW; END $$")).execute(&db).await.unwrap();
            sqlx::query(&format!("CREATE TRIGGER {hook} BEFORE INSERT OR UPDATE ON verification_tokens FOR EACH ROW EXECUTE FUNCTION {hook}()")).execute(&db).await.unwrap();
        }
        let response = app.post_json("/auth/register",json!({"username":name,"email":format!("{name}@example.test"),"password":"ValidPass123!"})).await;
        if failure == "token-store" {
            sqlx::query(&format!("DROP TRIGGER {hook} ON verification_tokens"))
                .execute(&db)
                .await
                .unwrap();
            sqlx::query(&format!("DROP FUNCTION {hook}()"))
                .execute(&db)
                .await
                .unwrap();
        }
        assert_eq!(response.status(), 500);
        assert!(
            queries::users::get_by_username(&db, &name)
                .await
                .unwrap()
                .is_none()
        );
        let expected = if failure == "subject" {
            vec!["AdminCreateUser", "AdminDeleteUser"]
        } else {
            vec!["AdminCreateUser", "AdminSetUserPassword", "AdminDeleteUser"]
        };
        assert_eq!(*calls.lock().unwrap(), expected);
        service.abort();
    }
}

#[tokio::test]
async fn verification_is_idempotent_for_confirmed_users_and_provider_failures_roll_back_token_use()
{
    use axum::{
        Json, Router,
        http::{HeaderMap, StatusCode},
    };
    use serde_json::json;
    use std::sync::{Arc, Mutex};
    for mode in ["confirmed", "unconfirmed", "get-failure", "confirm-failure"] {
        let calls = Arc::new(Mutex::new(Vec::new()));
        let seen = calls.clone();
        let router = Router::new().fallback(move |headers: HeaderMap| {
            let seen = seen.clone();
            async move {
                let target = headers.get("x-amz-target").unwrap().to_str().unwrap().rsplit('.').next().unwrap().to_owned();
                seen.lock().unwrap().push(target.clone());
                match target.as_str() {
                    "AdminGetUser" if mode == "get-failure" => (StatusCode::BAD_REQUEST, Json(json!({"__type":"UserNotFoundException","message":"fixture missing"}))),
                    "AdminGetUser" => (StatusCode::OK, Json(json!({"Username":"fixture","UserStatus":if mode == "confirmed" {"CONFIRMED"} else {"UNCONFIRMED"},"UserAttributes":[]}))),
                    "AdminConfirmSignUp" if mode == "confirm-failure" => (StatusCode::BAD_REQUEST, Json(json!({"__type":"NotAuthorizedException","message":"fixture failure"}))),
                    "AdminConfirmSignUp" => (StatusCode::OK, Json(json!({}))),
                    _ => panic!("unexpected provider operation"),
                }
            }
        });
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let endpoint = format!("http://{}", listener.local_addr().unwrap());
        let service = tokio::spawn(async move { axum::serve(listener, router).await.unwrap() });
        let mut cfg = common::test_config();
        cfg.cognito_endpoint_url = Some(endpoint);
        let app = common::TestApp::with_config(cfg.clone()).await;
        let db = dal_db::connect(&cfg.database_url).await.unwrap();
        let id = uuid::Uuid::new_v4();
        let name = format!("confirm-{}", &id.simple().to_string()[..12]);
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
        let raw = format!("local-verify-{}", uuid::Uuid::new_v4());
        queries::users::upsert_verification_token(
            &db,
            id,
            &dal_auth::hash_token(&raw),
            "email_verify",
            chrono::Utc::now() + chrono::Duration::hours(1),
        )
        .await
        .unwrap();
        let response = app.get(&format!("/auth/verify-email?token={raw}")).await;
        let success = matches!(mode, "confirmed" | "unconfirmed");
        assert_eq!(response.status(), if success { 200 } else { 500 });
        assert_eq!(
            queries::users::get_by_id(&db, id)
                .await
                .unwrap()
                .unwrap()
                .email_verified,
            success
        );
        let used:bool=sqlx::query_scalar("SELECT used_at IS NOT NULL FROM verification_tokens WHERE user_id=$1 AND kind='email_verify'").bind(id).fetch_one(&db).await.unwrap();
        assert_eq!(used, success);
        let expected = if matches!(mode, "confirmed" | "get-failure") {
            vec!["AdminGetUser"]
        } else {
            vec!["AdminGetUser", "AdminConfirmSignUp"]
        };
        assert_eq!(*calls.lock().unwrap(), expected);
        service.abort();
    }
}

#[tokio::test]
async fn reset_storage_failure_keeps_generic_response_without_queueing_an_invalid_link() {
    use axum::{Router, http::StatusCode};
    use std::sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    };
    let requests = Arc::new(AtomicUsize::new(0));
    let observed = requests.clone();
    let router = Router::new().fallback(move || {
        let observed = observed.clone();
        async move {
            observed.fetch_add(1, Ordering::SeqCst);
            StatusCode::OK
        }
    });
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let endpoint = format!("http://{}", listener.local_addr().unwrap());
    let service = tokio::spawn(async move { axum::serve(listener, router).await.unwrap() });
    let mut cfg = common::test_config();
    cfg.sqs_endpoint_url = Some(endpoint.clone());
    cfg.sqs_queue_url = format!("{endpoint}/000000000000/dal-jobs-test.fifo");
    let app = common::TestApp::with_config(cfg.clone()).await;
    let db = dal_db::connect(&cfg.database_url).await.unwrap();
    let id = uuid::Uuid::new_v4();
    let name = format!("reset-{}", &id.simple().to_string()[..12]);
    let email = format!("{name}@example.test");
    queries::users::create(&db, id, &name, &email, &id.to_string(), None)
        .await
        .unwrap();
    let constraint = format!("audit_reset_{}", id.simple());
    sqlx::query(&format!("ALTER TABLE verification_tokens ADD CONSTRAINT {constraint} CHECK (user_id <> '{id}'::uuid OR kind <> 'password_reset')")).execute(&db).await.unwrap();
    let response = app
        .post_json("/auth/forgot-password", serde_json::json!({"email":email}))
        .await;
    sqlx::query(&format!(
        "ALTER TABLE verification_tokens DROP CONSTRAINT {constraint}"
    ))
    .execute(&db)
    .await
    .unwrap();
    assert_eq!(response.status(), 200);
    assert_eq!(requests.load(Ordering::SeqCst), 0);
    let count: i64 =
        sqlx::query_scalar("SELECT count(*) FROM verification_tokens WHERE user_id=$1")
            .bind(id)
            .fetch_one(&db)
            .await
            .unwrap();
    assert_eq!(count, 0);
    service.abort();
}
