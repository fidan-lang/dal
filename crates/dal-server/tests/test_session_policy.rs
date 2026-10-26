mod common;

use axum::{
    Json, Router,
    body::{Body, to_bytes},
    http::{HeaderMap, Method, Request, StatusCode, header},
    response::Response,
};
use dal_auth::{YANK_SCOPE, generate_api_token};
use dal_db::{
    models::{ApiToken, User},
    queries,
};
use serde_json::{Value, json};
use std::sync::{
    Arc,
    atomic::{AtomicUsize, Ordering},
};
use uuid::Uuid;

async fn user(db: &sqlx::PgPool, verified: bool) -> User {
    let id = Uuid::new_v4();
    let name = format!("session-{}", &id.simple().to_string()[..16]);
    queries::users::create(
        db,
        id,
        &name,
        &format!("{name}@example.test"),
        &id.to_string(),
        None,
    )
    .await
    .unwrap();
    if verified {
        queries::users::set_email_verified(db, id).await.unwrap();
    }
    queries::users::get_by_id(db, id).await.unwrap().unwrap()
}
async fn token(db: &sqlx::PgPool, id: Uuid, scopes: &[String]) -> (ApiToken, String) {
    let raw = generate_api_token();
    let row = queries::tokens::create(db, id, "fixture", &raw.hash, &raw.prefix, scopes, None)
        .await
        .unwrap();
    (row, raw.raw)
}
async fn package(db: &sqlx::PgPool, owner: Uuid) -> Uuid {
    let id = Uuid::new_v4();
    queries::packages::create(
        db,
        queries::packages::NewPackage {
            id,
            name: &format!("session-{}", id.simple()),
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
    queries::packages::add_owner(db, id, owner, "owner", None)
        .await
        .unwrap();
    id
}
async fn invite(db: &sqlx::PgPool, package: Uuid, invitee: Uuid, inviter: Uuid) -> Uuid {
    queries::owner_invites::upsert(
        db,
        package,
        invitee,
        inviter,
        &format!("fixture-{}", Uuid::new_v4()),
        "collaborator",
        chrono::Utc::now() + chrono::Duration::hours(1),
    )
    .await
    .unwrap()
    .id
}

#[tokio::test]
async fn api_bearers_cannot_manage_credentials_invites_or_cognito_sessions() {
    let calls = Arc::new(AtomicUsize::new(0));
    let seen = calls.clone();
    let provider = Router::new().fallback(move |_headers: HeaderMap| {
        let seen = seen.clone();
        async move {
            seen.fetch_add(1, Ordering::SeqCst);
            (StatusCode::OK, Json(json!({})))
        }
    });
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let endpoint = format!("http://{}", listener.local_addr().unwrap());
    let provider_task = tokio::spawn(async move { axum::serve(listener, provider).await.unwrap() });
    let mut cfg = common::test_config();
    cfg.cognito_endpoint_url = Some(endpoint);
    let app = common::TestApp::with_config(cfg.clone()).await;
    let db = dal_db::connect(&cfg.database_url).await.unwrap();
    let owner = user(&db, true).await;
    for verified in [true, false] {
        let actor = user(&db, verified).await;
        for scopes in [
            vec![YANK_SCOPE.into()],
            vec![
                dal_auth::OWNER_SCOPE.into(),
                dal_auth::USER_WRITE_SCOPE.into(),
            ],
        ] {
            let (bearer, raw) = token(&db, actor.id, &scopes).await;
            let (sibling, _) = token(&db, actor.id, &[YANK_SCOPE.into()]).await;
            let accept = invite(&db, package(&db, owner.id).await, actor.id, owner.id).await;
            let decline = invite(&db, package(&db, owner.id).await, actor.id, owner.id).await;
            let before: Value =
                sqlx::query_scalar("SELECT row_to_json(t) FROM api_tokens t WHERE id=$1")
                    .bind(bearer.id)
                    .fetch_one(&db)
                    .await
                    .unwrap();
            let mut outcomes = Vec::new();
            for (method, path, body) in [
                (Method::GET, "/tokens".to_string(), json!({})),
                (
                    Method::POST,
                    "/tokens".to_string(),
                    json!({"name":"forbidden","scopes":["owner"]}),
                ),
                (Method::DELETE, format!("/tokens/{}", sibling.id), json!({})),
                (Method::GET, "/owners/invites".to_string(), json!({})),
                (
                    Method::POST,
                    format!("/owners/invites/{accept}/accept"),
                    json!({}),
                ),
                (
                    Method::POST,
                    format!("/owners/invites/{decline}/decline"),
                    json!({}),
                ),
                (Method::POST, "/auth/logout".to_string(), json!({})),
                (Method::DELETE, format!("/tokens/{}", bearer.id), json!({})),
            ] {
                let status = app
                    .request_json(method, &path, body, Some(&raw))
                    .await
                    .status();
                outcomes.push((path, status.as_u16()));
            }
            assert!(
                outcomes.iter().all(|(_, status)| *status == 403),
                "forbidden session policy: {outcomes:?}"
            );
            assert_eq!(
                calls.load(Ordering::SeqCst),
                0,
                "denial must not call Cognito"
            );
            let after: Value =
                sqlx::query_scalar("SELECT row_to_json(t) FROM api_tokens t WHERE id=$1")
                    .bind(bearer.id)
                    .fetch_one(&db)
                    .await
                    .unwrap();
            assert_eq!(
                before, after,
                "denied administration must not even touch token usage"
            );
            assert!(
                queries::tokens::get_by_hash(&db, &sibling.token_hash)
                    .await
                    .unwrap()
                    .is_some()
            );
            assert!(
                queries::owner_invites::get_pending_for_user(&db, accept, actor.id)
                    .await
                    .unwrap()
                    .is_some()
            );
            assert!(
                queries::owner_invites::get_pending_for_user(&db, decline, actor.id)
                    .await
                    .unwrap()
                    .is_some()
            );
            assert_eq!(
                queries::tokens::list_for_user(&db, actor.id)
                    .await
                    .unwrap()
                    .len(),
                2 * (if scopes.len() == 1 { 1 } else { 2 })
            );
        }
    }
    provider_task.abort();
}

#[tokio::test]
async fn api_token_serialization_never_contains_verifier_material() {
    let _app = common::TestApp::spawn().await;
    let db = dal_db::connect(&common::test_config().database_url)
        .await
        .unwrap();
    let actor = user(&db, true).await;
    let (row, raw) = token(&db, actor.id, &[YANK_SCOPE.into()]).await;
    let json = serde_json::to_value(&row).unwrap();
    assert!(
        json.get("token_hash").is_none(),
        "internal verifier must never serialize"
    );
    assert!(!json.to_string().contains(&raw));
}

#[tokio::test]
async fn verified_sessions_manage_only_their_tokens_without_exposing_hashes() {
    let app = common::TestApp::spawn().await;
    let db = dal_db::connect(&common::test_config().database_url)
        .await
        .unwrap();
    let actor = user(&db, true).await;
    let foreign = user(&db, true).await;
    let (foreign_token, _) = token(&db, foreign.id, &[YANK_SCOPE.into()]).await;
    let jwt = app.interactive_token(&db, actor.id).await;
    let (status, created) = common::TestApp::unpack(
        app.request_json(
            Method::POST,
            "/tokens",
            json!({"name":"interactive","scopes":["yank"]}),
            Some(&jwt),
        )
        .await,
    )
    .await;
    assert_eq!(status, 201);
    assert!(created["token"].as_str().unwrap().starts_with("dal_"));
    assert!(created["meta"].get("token_hash").is_none());
    let id = created["meta"]["id"].as_str().unwrap();
    let (status, listed) = common::TestApp::unpack(
        app.request_json(Method::GET, "/tokens", json!({}), Some(&jwt))
            .await,
    )
    .await;
    assert_eq!(status, 200);
    assert_eq!(listed.as_array().unwrap().len(), 1);
    assert!(listed[0].get("token_hash").is_none());
    assert!(listed[0].get("token").is_none());
    assert!(
        app.request_json(
            Method::DELETE,
            &format!("/tokens/{}", foreign_token.id),
            json!({}),
            Some(&jwt)
        )
        .await
        .status()
        .is_client_error()
    );
    assert!(
        queries::tokens::get_by_hash(&db, &foreign_token.token_hash)
            .await
            .unwrap()
            .is_some()
    );
    assert_eq!(
        app.request_json(
            Method::DELETE,
            &format!("/tokens/{id}"),
            json!({}),
            Some(&jwt)
        )
        .await
        .status(),
        200
    );
    for credential in [None, Some("not-a-jwt")] {
        assert_eq!(
            app.request_json(Method::GET, "/tokens", json!({}), credential)
                .await
                .status(),
            401
        );
    }
    sqlx::query("UPDATE users SET email_verified=FALSE WHERE id=$1")
        .bind(actor.id)
        .execute(&db)
        .await
        .unwrap();
    assert_eq!(
        app.request_json(Method::GET, "/tokens", json!({}), Some(&jwt))
            .await
            .status(),
        403
    );
}

#[tokio::test]
async fn unverified_interactive_admin_cannot_mutate_packages() {
    let app = common::TestApp::spawn().await;
    let db = dal_db::connect(&common::test_config().database_url)
        .await
        .unwrap();
    let actor = user(&db, false).await;
    sqlx::query("UPDATE users SET is_admin=TRUE WHERE id=$1")
        .bind(actor.id)
        .execute(&db)
        .await
        .unwrap();
    let jwt = app.interactive_token(&db, actor.id).await;
    let owner = user(&db, true).await;
    let package_id = package(&db, owner.id).await;
    let package = queries::packages::get_by_id(&db, package_id)
        .await
        .unwrap()
        .unwrap();
    let status = app
        .request_json(
            Method::DELETE,
            &format!("/admin/packages/{}", package.name),
            json!({}),
            Some(&jwt),
        )
        .await
        .status();
    assert_eq!(
        status, 403,
        "unverified sessions must not bypass the mutation policy through admin privileges"
    );
    assert!(
        queries::packages::get_by_id(&db, package_id)
            .await
            .unwrap()
            .is_some()
    );
}

async fn start_signout_proxy(
    upstream: String,
    fail_signout: bool,
) -> (String, Arc<AtomicUsize>, tokio::task::JoinHandle<()>) {
    let signout_requests = Arc::new(AtomicUsize::new(0));
    let observed = signout_requests.clone();
    let client = reqwest::Client::new();
    let provider = Router::new().fallback(move |request: Request<Body>| {
        let observed = observed.clone();
        let upstream = upstream.clone();
        let client = client.clone();
        async move {
            let (parts, body) = request.into_parts();
            let target = parts
                .headers
                .get("x-amz-target")
                .and_then(|value| value.to_str().ok())
                .and_then(|value| value.rsplit('.').next());
            if target == Some("AdminUserGlobalSignOut") {
                observed.fetch_add(1, Ordering::SeqCst);
                return Response::builder()
                    .status(if fail_signout {
                        StatusCode::BAD_GATEWAY
                    } else {
                        StatusCode::OK
                    })
                    .header(header::CONTENT_TYPE, "application/x-amz-json-1.1")
                    .body(Body::from(if fail_signout {
                        r#"{"__type":"ServiceUnavailableException","message":"fixture outage"}"#
                    } else {
                        "{}"
                    }))
                    .unwrap();
            }

            let body = to_bytes(body, 64 * 1024).await.unwrap();
            let mut forwarded = client.request(
                parts.method,
                format!(
                    "{upstream}{}",
                    parts.uri.path_and_query().map_or("/", |path| path.as_str())
                ),
            );
            for name in [
                header::CONTENT_TYPE,
                header::HeaderName::from_static("x-amz-target"),
            ] {
                if let Some(value) = parts.headers.get(&name) {
                    forwarded = forwarded.header(name, value);
                }
            }
            let response = forwarded.body(body).send().await.unwrap();
            let status = response.status();
            let content_type = response.headers().get(header::CONTENT_TYPE).cloned();
            let body = response.bytes().await.unwrap();
            let mut response = Response::builder().status(status);
            if let Some(content_type) = content_type {
                response = response.header(header::CONTENT_TYPE, content_type);
            }
            response.body(Body::from(body)).unwrap()
        }
    });
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let endpoint = format!("http://{}", listener.local_addr().unwrap());
    let task = tokio::spawn(async move { axum::serve(listener, provider).await.unwrap() });
    (endpoint, signout_requests, task)
}

#[tokio::test]
#[ignore = "requires cognito-local (run scripts/test-local.mjs --with-cognito)"]
async fn logout_returns_http_error_when_global_sign_out_fails() {
    let upstream = std::env::var("TEST_COGNITO_ENDPOINT_URL")
        .expect("cognito-local endpoint required for the forwarding proxy");
    let (endpoint, calls, proxy_task) = start_signout_proxy(upstream, true).await;
    let cfg = common::test_config();
    let app = common::TestApp::with_cognito_control_endpoint(cfg.clone(), Some(&endpoint)).await;
    let db = dal_db::connect(&cfg.database_url).await.unwrap();
    let actor = user(&db, true).await;
    let jwt = app.interactive_token(&db, actor.id).await;

    let response = app
        .request_json(Method::POST, "/auth/logout", json!({}), Some(&jwt))
        .await;
    assert_eq!(response.status(), StatusCode::INTERNAL_SERVER_ERROR);
    assert!(
        response.headers().get(header::SET_COOKIE).is_none(),
        "a failed global logout must not send a success-shaped cookie response"
    );
    let body = common::TestApp::json_body(response).await;
    assert_eq!(body["error"]["code"], "COGNITO_ERROR");
    assert_eq!(calls.load(Ordering::SeqCst), 1);
    proxy_task.abort();
}

#[tokio::test]
#[ignore = "requires cognito-local (run scripts/test-local.mjs --with-cognito)"]
async fn logout_revokes_an_already_issued_jwt_at_the_dal_api() {
    // cognito-local does not implement AdminUserGlobalSignOut. Only that control
    // operation is simulated; real JWT issuance/JWKS and the Dal HTTP/DB path run.
    let upstream = std::env::var("TEST_COGNITO_ENDPOINT_URL").unwrap();
    let (endpoint, calls, proxy_task) = start_signout_proxy(upstream, false).await;
    let app =
        common::TestApp::with_cognito_control_endpoint(common::test_config(), Some(&endpoint))
            .await;
    let db = dal_db::connect(&common::test_config().database_url)
        .await
        .unwrap();
    let actor = user(&db, true).await;
    let jwt = app.interactive_token(&db, actor.id).await;

    assert_eq!(
        app.request_json(Method::GET, "/auth/me", json!({}), Some(&jwt))
            .await
            .status(),
        StatusCode::OK
    );
    assert_eq!(
        app.request_json(Method::POST, "/auth/logout", json!({}), Some(&jwt))
            .await
            .status(),
        StatusCode::OK
    );
    assert_eq!(
        app.request_json(Method::GET, "/auth/me", json!({}), Some(&jwt))
            .await
            .status(),
        StatusCode::UNAUTHORIZED,
        "offline JWT validation must enforce Dal's persisted logout cutoff"
    );
    assert_eq!(calls.load(Ordering::SeqCst), 1);
    assert!(
        queries::users::get_by_id(&db, actor.id)
            .await
            .unwrap()
            .unwrap()
            .session_invalid_before
            .is_some()
    );
    proxy_task.abort();
}

#[tokio::test]
#[ignore = "requires ephemeral JWT signing fixtures from scripts/test-local.mjs --with-cognito"]
async fn signed_jwt_expiration_signature_and_audience_are_enforced_over_http() {
    use jsonwebtoken::{Algorithm, EncodingKey, Header, encode};
    let key = EncodingKey::from_rsa_pem(std::env::var("TEST_JWT_PRIVATE_KEY").unwrap().as_bytes())
        .unwrap();
    let jwks = json!({"keys":[{"kty":"RSA","alg":"RS256","use":"sig","kid":"local-reaudit-key",
        "n":std::env::var("TEST_JWT_JWK_N").unwrap(),"e":std::env::var("TEST_JWT_JWK_E").unwrap()}]});
    let refresh_result = Arc::new(std::sync::Mutex::new(String::new()));
    let result_for_provider = refresh_result.clone();
    let provider = Router::new().fallback(move |headers: HeaderMap| {
        let jwks = jwks.clone();
        let result = result_for_provider.clone();
        async move {
            if headers.contains_key("x-amz-target") {
                Json(json!({"AuthenticationResult": {
                    "IdToken": result.lock().unwrap().clone(), "AccessToken": "fixture-access"
                }}))
            } else {
                Json(jwks)
            }
        }
    });
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let endpoint = format!("http://{}", listener.local_addr().unwrap());
    let server = tokio::spawn(async move {
        axum::serve(listener, provider).await.unwrap();
    });
    let mut cfg = common::test_config();
    cfg.cognito_endpoint_url = Some(endpoint.clone());
    let app = common::TestApp::with_config(cfg.clone()).await;
    let db = dal_db::connect(&cfg.database_url).await.unwrap();
    let actor = user(&db, true).await;
    let now = chrono::Utc::now().timestamp();
    let claims = json!({"sub":actor.cognito_sub,"aud":cfg.cognito_client_id,"iss":format!("{endpoint}/{}",cfg.cognito_user_pool_id),"iat":now,"exp":now+3600,"token_use":"id"});
    let mut header = Header::new(Algorithm::RS256);
    header.kid = Some("local-reaudit-key".into());
    let valid = encode(&header, &claims, &key).unwrap();
    assert_eq!(
        app.request_json(Method::GET, "/auth/me", json!({}), Some(&valid))
            .await
            .status(),
        StatusCode::OK
    );
    for (field, bad_value) in [
        ("exp", json!(now - 3600)),
        ("aud", json!("another-client")),
        ("iss", json!("https://another-issuer.test")),
        ("token_use", json!("other")),
        ("sub", json!("")),
    ] {
        let mut invalid = claims.clone();
        invalid[field] = bad_value;
        let token = encode(&header, &invalid, &key).unwrap();
        assert_eq!(
            app.request_json(Method::GET, "/auth/me", json!({}), Some(&token))
                .await
                .status(),
            StatusCode::UNAUTHORIZED,
            "signed invalid {field} must be rejected"
        );
    }
    // A refresh has a new iat, but belongs to the original authentication epoch.
    queries::users::advance_session_invalid_before(
        &db,
        actor.id,
        chrono::Utc::now() - chrono::Duration::seconds(10),
    )
    .await
    .unwrap();
    let mut refreshed = claims.clone();
    refreshed["auth_time"] = json!(now - 100);
    let stale_session = encode(&header, &refreshed, &key).unwrap();
    assert_eq!(
        app.request_json(Method::GET, "/auth/me", json!({}), Some(&stale_session))
            .await
            .status(),
        StatusCode::UNAUTHORIZED,
        "a fresh iat must not resurrect a revoked authentication epoch"
    );
    *refresh_result.lock().unwrap() = stale_session;
    let response = app
        .request_json(
            Method::POST,
            "/auth/refresh",
            json!({"refresh_token":"fixture-only"}),
            None,
        )
        .await;
    assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
    assert!(
        response.headers().get(header::SET_COOKIE).is_none(),
        "revoked refresh must not issue a cookie"
    );
    refreshed["auth_time"] = json!(now);
    let new_session = encode(&header, &refreshed, &key).unwrap();
    assert_eq!(
        app.request_json(Method::GET, "/auth/me", json!({}), Some(&new_session))
            .await
            .status(),
        StatusCode::OK
    );
    *refresh_result.lock().unwrap() = new_session;
    let response = app
        .request_json(
            Method::POST,
            "/auth/refresh",
            json!({"refresh_token":"fixture-only"}),
            None,
        )
        .await;
    assert_eq!(response.status(), StatusCode::OK);
    assert!(response.headers().get(header::SET_COOKIE).is_some());
    refreshed.as_object_mut().unwrap().remove("auth_time");
    let missing_epoch = encode(&header, &refreshed, &key).unwrap();
    assert_eq!(
        app.request_json(Method::GET, "/auth/me", json!({}), Some(&missing_epoch))
            .await
            .status(),
        StatusCode::UNAUTHORIZED,
        "once a revocation exists, an absent original-authentication epoch must fail closed"
    );
    let mut forged = valid.into_bytes();
    let signature = forged.iter().rposition(|byte| *byte == b'.').unwrap() + 1;
    forged[signature] = if forged[signature] == b'A' {
        b'B'
    } else {
        b'A'
    };
    assert_eq!(
        app.request_json(
            Method::GET,
            "/auth/me",
            json!({}),
            Some(&String::from_utf8(forged).unwrap())
        )
        .await
        .status(),
        StatusCode::UNAUTHORIZED
    );
    server.abort();
}

#[tokio::test]
#[ignore = "requires cognito-local from scripts/test-local.mjs --with-cognito"]
async fn interactive_invites_enforce_recipient_expiry_replay_and_concurrent_claims() {
    let app = common::TestApp::spawn().await;
    let db = dal_db::connect(&common::test_config().database_url)
        .await
        .unwrap();
    let owner = user(&db, true).await;
    let recipient = user(&db, true).await;
    let stranger = user(&db, true).await;
    let recipient_jwt = app.interactive_token(&db, recipient.id).await;
    let stranger_jwt = app.interactive_token(&db, stranger.id).await;
    let pkg = package(&db, owner.id).await;
    let id = invite(&db, pkg, recipient.id, owner.id).await;
    let accept = format!("/owners/invites/{id}/accept");
    assert_eq!(
        app.request_json(Method::POST, &accept, json!({}), Some(&stranger_jwt))
            .await
            .status(),
        StatusCode::UNPROCESSABLE_ENTITY
    );
    assert!(
        !queries::packages::is_member(&db, pkg, recipient.id)
            .await
            .unwrap()
    );
    sqlx::query("UPDATE owner_invites SET expires_at=now()-interval '1 second' WHERE id=$1")
        .bind(id)
        .execute(&db)
        .await
        .unwrap();
    assert_eq!(
        app.request_json(Method::POST, &accept, json!({}), Some(&recipient_jwt))
            .await
            .status(),
        StatusCode::UNPROCESSABLE_ENTITY
    );
    assert!(
        !queries::packages::is_member(&db, pkg, recipient.id)
            .await
            .unwrap()
    );
    let id = invite(&db, pkg, recipient.id, owner.id).await;
    let accept = format!("/owners/invites/{id}/accept");
    let (one, two) = tokio::join!(
        app.request_json(Method::POST, &accept, json!({}), Some(&recipient_jwt)),
        app.request_json(Method::POST, &accept, json!({}), Some(&recipient_jwt))
    );
    let mut statuses = [one.status().as_u16(), two.status().as_u16()];
    statuses.sort();
    assert_eq!(statuses, [200, 422]);
    assert!(
        queries::packages::is_member(&db, pkg, recipient.id)
            .await
            .unwrap()
    );
    assert_eq!(
        app.request_json(Method::POST, &accept, json!({}), Some(&recipient_jwt))
            .await
            .status(),
        StatusCode::UNPROCESSABLE_ENTITY
    );
    assert_eq!(
        app.request_json(
            Method::POST,
            &format!("/owners/invites/{id}/decline"),
            json!({}),
            Some(&recipient_jwt)
        )
        .await
        .status(),
        StatusCode::UNPROCESSABLE_ENTITY
    );
    let count: (i64,) =
        sqlx::query_as("SELECT count(*) FROM package_owners WHERE package_id=$1 AND user_id=$2")
            .bind(pkg)
            .bind(recipient.id)
            .fetch_one(&db)
            .await
            .unwrap();
    assert_eq!(count.0, 1);
}
