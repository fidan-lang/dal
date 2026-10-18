mod common;
use axum::http::Method;
use dal_auth::{PUBLISH_NEW_SCOPE, YANK_SCOPE, generate_api_token};
use dal_db::{connect, queries};
use serde_json::json;
use uuid::Uuid;

async fn identity(verified: bool) -> (sqlx::PgPool, Uuid, String) {
    let db = connect(
        &std::env::var("TEST_DATABASE_URL")
            .unwrap_or_else(|_| "postgres://dal_test:test@127.0.0.1:5433/dal_test".into()),
    )
    .await
    .unwrap();
    let id = Uuid::new_v4();
    let name = format!("security-{}", id.simple());
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
    if verified {
        queries::users::set_email_verified(&db, id).await.unwrap();
    }
    let token = generate_api_token();
    queries::tokens::create(
        &db,
        id,
        "limited",
        &token.hash,
        &token.prefix,
        &[PUBLISH_NEW_SCOPE.into(), YANK_SCOPE.into()],
        None,
    )
    .await
    .unwrap();
    (db, id, token.raw)
}
#[tokio::test]
async fn publishing_token_cannot_mint_tokens_or_invoke_admin() {
    let app = common::TestApp::spawn().await;
    let (db, id, token) = identity(true).await;
    let response = app
        .request_json(
            Method::POST,
            "/tokens",
            json!({"name":"escalation","scopes":["owner"]}),
            Some(&token),
        )
        .await;
    assert_eq!(response.status(), 403);
    assert_eq!(
        queries::tokens::list_for_user(&db, id).await.unwrap().len(),
        1
    );
    sqlx::query("UPDATE users SET is_admin = TRUE WHERE id = $1")
        .bind(id)
        .execute(&db)
        .await
        .unwrap();
    assert_eq!(
        app.request_json(
            Method::DELETE,
            "/admin/packages/does-not-exist",
            json!({}),
            Some(&token)
        )
        .await
        .status(),
        403
    );
}
#[tokio::test]
async fn unverified_and_expired_tokens_cannot_mutate_registry() {
    let app = common::TestApp::spawn().await;
    let (db, id, token) = identity(false).await;
    assert_eq!(
        app.request_json(
            Method::PUT,
            "/packages/missing/versions/1.0.0/yank",
            json!({}),
            Some(&token)
        )
        .await
        .status(),
        403
    );
    sqlx::query(
        "UPDATE api_tokens SET expires_at = now() - interval '1 second' WHERE user_id = $1",
    )
    .bind(id)
    .execute(&db)
    .await
    .unwrap();
    assert_eq!(
        app.request_json(Method::GET, "/auth/me", json!({}), Some(&token))
            .await
            .status(),
        401
    );
}
#[tokio::test]
async fn upload_limit_is_explicit_and_not_axum_default_two_megabytes() {
    let app = common::TestApp::spawn().await;
    let (_, _, token) = identity(true).await;
    let response = app
        .publish_archive("upload-limit", &token, &vec![b'x'; 3 * 1024 * 1024])
        .await;
    let (status, body) = common::TestApp::unpack(response).await;
    assert_eq!(
        status, 422,
        "the complete upload should reach bounded gzip validation"
    );
    assert_eq!(body["error"]["code"], "ARCHIVE_INVALID");
    let response = app
        .publish_archive("upload-limit", &token, &vec![b'x'; 11 * 1024 * 1024])
        .await;
    assert!(matches!(response.status().as_u16(), 413 | 422));
}

#[tokio::test]
async fn failed_reset_does_not_consume_token() {
    let (db, id, _) = identity(true).await;
    let raw = format!("pr_local_failure_{}", Uuid::new_v4().simple());
    let hash = dal_auth::hash_token(&raw);
    queries::users::upsert_verification_token(
        &db,
        id,
        &hash,
        "password_reset",
        chrono::Utc::now() + chrono::Duration::hours(1),
    )
    .await
    .unwrap();
    let mut cfg = common::test_config();
    cfg.cognito_endpoint_url = Some("http://127.0.0.1:1".into());
    let app = common::TestApp::with_config(cfg).await;
    let response = app
        .post_json(
            "/auth/reset-password",
            json!({"token":raw,"new_password":"RetryPassword123!"}),
        )
        .await;
    let (status, body) = common::TestApp::unpack(response).await;
    assert_eq!(status, 500);
    assert_eq!(body["error"]["message"], "internal server error");
    assert_eq!(
        queries::users::consume_verification_token(&db, &hash, "password_reset")
            .await
            .unwrap(),
        Some(id)
    );
}

#[tokio::test]
async fn account_cleanup_rechecks_verification_after_row_lock() {
    let _app = common::TestApp::spawn().await;
    let (db, id, _) = identity(false).await;
    sqlx::query("UPDATE users SET created_at = now() - interval '30 days' WHERE id = $1")
        .bind(id)
        .execute(&db)
        .await
        .unwrap();
    let cutoff = chrono::Utc::now() - chrono::Duration::days(7);
    let mut verifying = db.begin().await.unwrap();
    queries::users::set_email_verified(&mut *verifying, id)
        .await
        .unwrap();
    let cleaning_db = db.clone();
    let cleanup = tokio::spawn(async move {
        let mut transaction = cleaning_db.begin().await.unwrap();
        queries::users::lock_stale_unverified(&mut *transaction, id, cutoff)
            .await
            .unwrap()
    });
    verifying.commit().await.unwrap();
    assert!(cleanup.await.unwrap().is_none());
    assert!(
        queries::users::get_by_id(&db, id)
            .await
            .unwrap()
            .unwrap()
            .email_verified
    );
}

#[tokio::test]
async fn invalid_profile_urls_are_rejected_without_mutating_persisted_profile() {
    let app = common::TestApp::spawn().await;
    let (db, id, token) = identity(true).await;
    sqlx::query("UPDATE api_tokens SET scopes = $1 WHERE user_id = $2")
        .bind(vec![dal_auth::USER_WRITE_SCOPE])
        .bind(id)
        .execute(&db)
        .await
        .unwrap();
    for field in ["website", "avatar_url"] {
        for value in [
            "javascript:alert(1)",
            "not a URL",
            "data:text/html,unsafe",
            "https://user:password@example.test",
        ] {
            assert_eq!(
                app.request_json(
                    Method::PATCH,
                    "/users/me/profile",
                    json!({field:value}),
                    Some(&token)
                )
                .await
                .status(),
                422
            );
        }
    }
    let user = queries::users::get_by_id(&db, id).await.unwrap().unwrap();
    assert!(user.website.is_none());
    assert!(user.avatar_url.is_none());
    assert_eq!(
        app.request_json(
            Method::PATCH,
            "/users/me/profile",
            json!({"website":"https://example.test/profile"}),
            Some(&token)
        )
        .await
        .status(),
        200
    );
    assert_eq!(
        queries::users::get_by_id(&db, id)
            .await
            .unwrap()
            .unwrap()
            .website
            .as_deref(),
        Some("https://example.test/profile")
    );
}
