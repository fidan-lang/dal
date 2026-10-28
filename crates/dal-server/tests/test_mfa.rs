mod common;
use axum::{
    Json, Router,
    body::{Body, Bytes},
    http::{HeaderMap, Request, StatusCode},
    routing::post,
};
use serde_json::{Value, json};
use std::sync::{
    Arc, Mutex,
    atomic::{AtomicBool, AtomicUsize, Ordering},
};
use uuid::Uuid;

#[tokio::test]
async fn mfa_endpoints_require_interactive_auth_and_never_accept_api_tokens() {
    let app = common::TestApp::spawn().await;
    for path in [
        "/auth/mfa/status",
        "/auth/mfa/setup",
        "/auth/mfa/verify",
        "/auth/mfa/disable",
    ] {
        let method = if path.ends_with("status") {
            "GET"
        } else {
            "POST"
        };
        let request = Request::builder()
            .method(method)
            .uri(path)
            .header("content-type", "application/json")
            .body(Body::from(r#"{"password":"fixture","code":"123456"}"#))
            .unwrap();
        assert_eq!(
            app.raw_request(request).await.status(),
            StatusCode::UNAUTHORIZED,
            "{path}"
        );
        let request = Request::builder()
            .method(method)
            .uri(path)
            .header("content-type", "application/json")
            .header(
                "authorization",
                format!("Bearer {}", dal_auth::generate_api_token().raw),
            )
            .body(Body::from(r#"{"password":"fixture","code":"123456"}"#))
            .unwrap();
        assert_eq!(
            app.raw_request(request).await.status(),
            StatusCode::FORBIDDEN,
            "{path}"
        );
    }
    let response = app
        .post_json("/auth/mfa/challenge", json!({"code":"123456"}))
        .await;
    assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
    assert_eq!(response.headers()["cache-control"], "no-store");
}
fn pending(response: &axum::response::Response, name: &str) -> String {
    response
        .headers()
        .get_all("set-cookie")
        .iter()
        .filter_map(|v| v.to_str().ok())
        .find_map(|v| {
            v.split(';')
                .next()
                .filter(|v| v.starts_with(&format!("{name}=")))
                .map(str::to_owned)
        })
        .unwrap()
}
fn no_session(response: &axum::response::Response) {
    for value in response.headers().get_all("set-cookie").iter() {
        let value = value.to_str().unwrap();
        assert!(!value.starts_with("dal_access_token="));
        assert!(!value.starts_with("dal_refresh_token="));
        assert!(value.contains("HttpOnly") && value.contains("SameSite=Strict"));
    }
    assert_eq!(response.headers()["cache-control"], "no-store");
}
#[tokio::test]
async fn cognito_managed_setup_challenge_replay_disable_and_downgrade_guard() {
    let baseline = common::TestApp::spawn().await;
    let db = dal_db::connect(&common::test_config().database_url)
        .await
        .unwrap();
    let id = Uuid::new_v4();
    let name = format!("mfa{}", &id.simple().to_string()[..16]);
    dal_db::queries::users::create(
        &db,
        id,
        &name,
        &format!("{name}@example.test"),
        &id.to_string(),
        None,
    )
    .await
    .unwrap();
    dal_db::queries::users::set_email_verified(&db, id)
        .await
        .unwrap();
    let jwt = baseline.interactive_token(&db, id).await;
    let identity = Arc::new(Mutex::new(jwt.clone()));
    let enabled = Arc::new(AtomicBool::new(false));
    let calls = Arc::new(AtomicUsize::new(0));
    let (identity2, enabled2, calls2) = (identity.clone(), enabled.clone(), calls.clone());
    let api=Router::new().route("/",post(move |headers:HeaderMap,raw:Bytes| {
        let body:Value=serde_json::from_slice(&raw).unwrap();
        let (identity,enabled,calls)=(identity2.clone(),enabled2.clone(),calls2.clone());
        async move {
            let action=headers["x-amz-target"].to_str().unwrap().split('.').next_back().unwrap();
            let tokens=||json!({"AuthenticationResult":{"AccessToken":"fixture-access","IdToken":identity.lock().unwrap().clone(),"RefreshToken":"fixture-refresh"}});
            let challenge=||json!({"ChallengeName":"SOFTWARE_TOKEN_MFA","Session":"fixture-private-session","ChallengeParameters":{"USERNAME":body["AuthParameters"]["USERNAME"]}});
            let fail=|| (StatusCode::BAD_REQUEST,Json(json!({"__type":"NotAuthorizedException","message":"private synthetic provider detail"})));
            match action {
                "AdminGetUser"=>(StatusCode::OK,Json(json!({"Username":"fixture","UserMFASettingList":if enabled.load(Ordering::SeqCst){vec!["SOFTWARE_TOKEN_MFA"]}else{vec![]}}))),
                "InitiateAuth"=>{
                    if body["AuthFlow"]=="REFRESH_TOKEN_AUTH" {
                        return if body["AuthParameters"]["REFRESH_TOKEN"]=="fixture-refresh" {
                            (StatusCode::OK,Json(tokens()))
                        } else { fail() };
                    }
                    if body["AuthParameters"]["PASSWORD"]!="LocalFixturePass123!" {return fail();}
                    (StatusCode::OK,Json(if enabled.load(Ordering::SeqCst){challenge()}else{tokens()}))
                },
                "RespondToAuthChallenge"=>{
                    calls.fetch_add(1,Ordering::SeqCst);
                    assert_eq!(body["ChallengeName"],"SOFTWARE_TOKEN_MFA");
                    if body["Session"]!="fixture-private-session" || body["ChallengeResponses"]["SOFTWARE_TOKEN_MFA_CODE"]!="123456" {return fail();}
                    (StatusCode::OK,Json(tokens()))
                },
                "AssociateSoftwareToken"=>{assert_eq!(body["AccessToken"],"fixture-access");(StatusCode::OK,Json(json!({"SecretCode":"JBSWY3DPEHPK3PXP"})))},
                "VerifySoftwareToken"=>(StatusCode::OK,Json(json!({"Status":if body["UserCode"]=="654321"{"SUCCESS"}else{"ERROR"}}))),
                "SetUserMFAPreference"=>{enabled.store(body["SoftwareTokenMfaSettings"]["Enabled"].as_bool().unwrap_or(false),Ordering::SeqCst);(StatusCode::OK,Json(json!({})))},
                "AdminUserGlobalSignOut"=>(StatusCode::OK,Json(json!({}))),
                _=>panic!("unexpected synthetic Cognito operation: {action}"),
            }
        }
    }));
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let endpoint = format!("http://{}", listener.local_addr().unwrap());
    let provider = tokio::spawn(async move {
        axum::serve(listener, api).await.unwrap();
    });
    let app =
        common::TestApp::with_cognito_control_endpoint(common::test_config(), Some(&endpoint))
            .await;
    let request = |path: &str, body: Value, cookie: Option<&str>, jwt: Option<&str>| {
        let mut r = Request::builder()
            .method("POST")
            .uri(path)
            .header("content-type", "application/json");
        if let Some(cookie) = cookie {
            r = r.header("cookie", cookie);
        }
        if let Some(jwt) = jwt {
            r = r.header("authorization", format!("Bearer {jwt}"));
        }
        r.body(Body::from(body.to_string())).unwrap()
    };
    let setup = app
        .raw_request(request(
            "/auth/mfa/setup",
            json!({"password":"LocalFixturePass123!"}),
            None,
            Some(&jwt),
        ))
        .await;
    assert_eq!(setup.status(), 200);
    no_session(&setup);
    let handle = pending(&setup, "dal_mfa_setup");
    let (_, body) = common::TestApp::unpack(setup).await;
    assert_eq!(body["secret"], "JBSWY3DPEHPK3PXP");
    assert!(body["uri"].as_str().unwrap().starts_with("otpauth://totp/"));
    let invalid = app
        .raw_request(request(
            "/auth/mfa/verify",
            json!({"code":"000000"}),
            Some(&handle),
            Some(&jwt),
        ))
        .await;
    assert_eq!(invalid.status(), 401);
    no_session(&invalid);
    assert!(!enabled.load(Ordering::SeqCst));
    let valid = app
        .raw_request(request(
            "/auth/mfa/verify",
            json!({"code":"654321"}),
            Some(&handle),
            Some(&jwt),
        ))
        .await;
    assert_eq!(valid.status(), 200);
    no_session(&valid);
    assert!(enabled.load(Ordering::SeqCst));
    assert!(dal_db::queries::users::mfa_required(&db, id).await.unwrap());
    // Cognito JWT iat/auth_time are integral; the previous full cutoff second is revoked.
    tokio::time::sleep(std::time::Duration::from_millis(1100)).await;
    let jwt = baseline.existing_interactive_token(&name).await;
    *identity.lock().unwrap() = jwt.clone();
    let direct = app
        .raw_request(
            Request::builder()
                .uri("/auth/me")
                .header("authorization", format!("Bearer {jwt}"))
                .body(Body::empty())
                .unwrap(),
        )
        .await;
    assert_eq!(
        direct.status(),
        401,
        "direct Cognito JWT cannot replace completed DAL MFA"
    );
    let premature = app
        .post_json("/auth/refresh", json!({"refresh_token":"fixture-refresh"}))
        .await;
    assert_eq!(premature.status(), 401);
    no_session(&premature);
    // A broken/disabled provider must not downgrade an enrolled DAL account.
    enabled.store(false, Ordering::SeqCst);
    let downgrade = app
        .post_json(
            "/auth/login",
            json!({"username":name,"password":"LocalFixturePass123!"}),
        )
        .await;
    assert_eq!(downgrade.status(), 401);
    no_session(&downgrade);
    enabled.store(true, Ordering::SeqCst);
    let login = app
        .post_json(
            "/auth/login",
            json!({"username":name,"password":"LocalFixturePass123!"}),
        )
        .await;
    assert_eq!(login.status(), 200);
    no_session(&login);
    let cookie = pending(&login, "dal_mfa_pending");
    let (_, body) = common::TestApp::unpack(login).await;
    assert_eq!(body["mfa_required"], true);
    let invalid = app
        .raw_request(request(
            "/auth/mfa/challenge",
            json!({"code":"000000"}),
            Some(&cookie),
            None,
        ))
        .await;
    assert_eq!(invalid.status(), 401);
    no_session(&invalid);
    let success = app
        .raw_request(request(
            "/auth/mfa/challenge",
            json!({"code":"123456"}),
            Some(&cookie),
            None,
        ))
        .await;
    assert_eq!(success.status(), 200);
    assert!(
        success
            .headers()
            .get_all("set-cookie")
            .iter()
            .any(|v| v.to_str().unwrap().starts_with("dal_access_token="))
    );
    let refreshed = app
        .post_json("/auth/refresh", json!({"refresh_token":"fixture-refresh"}))
        .await;
    assert_eq!(
        refreshed.status(),
        200,
        "only a completed MFA refresh credential is accepted"
    );
    let user = dal_db::queries::users::get_by_id(&db, id)
        .await
        .unwrap()
        .unwrap();
    assert!(
        dal_db::queries::mfa_sessions::authorized(&db, user.id, &dal_auth::hash_token(&jwt))
            .await
            .unwrap()
    );
    assert!(
        !dal_db::queries::mfa_sessions::authorized(
            &db,
            Uuid::new_v4(),
            &dal_auth::hash_token(&jwt)
        )
        .await
        .unwrap()
    );
    {
        let mut connection = db.acquire().await.unwrap();
        assert!(
            matches!(
                dal_db::queries::mfa_sessions::create(
                    &mut connection,
                    Uuid::new_v4(),
                    &dal_auth::hash_token("fixture-refresh")
                )
                .await,
                Err(dal_common::error::DalError::Unauthorized)
            ),
            "a refresh verifier must never be rebound to a different user"
        );
    }
    let count = calls.load(Ordering::SeqCst);
    let replay = app
        .raw_request(request(
            "/auth/mfa/challenge",
            json!({"code":"123456"}),
            Some(&cookie),
            None,
        ))
        .await;
    assert_eq!(replay.status(), 401);
    no_session(&replay);
    assert_eq!(calls.load(Ordering::SeqCst), count);
    let budget = app
        .post_json(
            "/auth/login",
            json!({"username":name,"password":"LocalFixturePass123!"}),
        )
        .await;
    let exhausted = pending(&budget, "dal_mfa_pending");
    for _ in 0..dal_server::mfa::MAX_ATTEMPTS {
        let failed = app
            .raw_request(request(
                "/auth/mfa/challenge",
                json!({"code":"000000"}),
                Some(&exhausted),
                None,
            ))
            .await;
        assert_eq!(failed.status(), 401);
        no_session(&failed);
    }
    let no_retry = app
        .raw_request(request(
            "/auth/mfa/challenge",
            json!({"code":"123456"}),
            Some(&exhausted),
            None,
        ))
        .await;
    assert_eq!(no_retry.status(), 401);
    no_session(&no_retry);

    let missing = app
        .raw_request(request(
            "/auth/mfa/disable",
            json!({"password":"LocalFixturePass123!"}),
            None,
            Some(&jwt),
        ))
        .await;
    assert_eq!(missing.status(), 401);
    assert!(enabled.load(Ordering::SeqCst));
    let disabled = app
        .raw_request(request(
            "/auth/mfa/disable",
            json!({"password":"LocalFixturePass123!","current_code":"123456"}),
            None,
            Some(&jwt),
        ))
        .await;
    assert_eq!(disabled.status(), 200);
    assert!(!enabled.load(Ordering::SeqCst));
    assert!(!dal_db::queries::users::mfa_required(&db, id).await.unwrap());
    provider.abort();
}
