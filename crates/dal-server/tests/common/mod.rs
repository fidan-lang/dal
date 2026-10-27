//! Shared test harness for dal-server integration tests.
//!
//! Usage in a test file:
//!   mod common;
//!   let app = common::TestApp::spawn().await;

#![allow(dead_code)]

use axum::body::{Body, to_bytes};
use axum::http::{Method, Request, Response, StatusCode};
use dal_server::{app::build_router, config::Config, state::AppState};
use tower::ServiceExt;

// ── Config ────────────────────────────────────────────────────────────────────

pub fn test_config() -> Config {
    let cfg = Config {
        totp_enabled: true,
        database_url: std::env::var("TEST_DATABASE_URL")
            .unwrap_or_else(|_| "postgres://dal_test:test@localhost:5433/dal_test".to_string()),
        trusted_proxies: Vec::new(),
        listen_addr: "127.0.0.1:0".to_string(),
        base_url: "http://localhost".to_string(),
        storage_bucket: "dal-test".to_string(),
        storage_endpoint_url: Some(
            std::env::var("TEST_STORAGE_ENDPOINT_URL")
                .unwrap_or_else(|_| "http://localhost:4567".to_string()),
        ),
        storage_access_key_id: Some("test".to_string()),
        storage_secret_access_key: Some("test".to_string()),
        storage_region: std::env::var("TEST_STORAGE_REGION")
            .unwrap_or_else(|_| "eu-central-1".to_string()),
        sqs_queue_url: std::env::var("TEST_SQS_QUEUE_URL").unwrap_or_else(|_| {
            "http://127.0.0.1:4567/000000000000/dal-jobs-test.fifo".to_string()
        }),
        sqs_endpoint_url: Some(
            std::env::var("TEST_SQS_ENDPOINT_URL")
                .unwrap_or_else(|_| "http://localhost:4567".to_string()),
        ),
        cognito_user_pool_id: std::env::var("TEST_COGNITO_POOL_ID")
            .unwrap_or_else(|_| "local_0dPm2L0N".to_string()),
        cognito_client_id: std::env::var("TEST_COGNITO_CLIENT_ID")
            .unwrap_or_else(|_| "bs5obcfdxmvh7g6ldnqmeahae".to_string()),
        cognito_endpoint_url: Some(
            std::env::var("TEST_COGNITO_ENDPOINT_URL")
                .unwrap_or_else(|_| "http://127.0.0.1:9229".to_string()),
        ),
        aws_region: std::env::var("AWS_REGION").unwrap_or_else(|_| "eu-central-1".to_string()),
        max_upload_bytes: 10_485_760,
        // Keep explicit coverage of the backwards-compatible signed-redirect mode.
        public_reads: dal_server::config::PublicReadConfig {
            mode: dal_server::config::DownloadMode::Redirect,
            ..Default::default()
        },
        mailjet_api_key: "test_key".to_string(),
        mailjet_secret_key: "test_secret".to_string(),
        mailjet_from_email: "test@dal.test".to_string(),
        mailjet_from_name: "Dal Tests".to_string(),
    };
    assert_isolated_config(&cfg);
    cfg
}

fn assert_isolated_config(cfg: &Config) {
    for (kind, value) in [
        ("database", cfg.database_url.as_str()),
        (
            "storage",
            cfg.storage_endpoint_url
                .as_deref()
                .expect("local storage endpoint required"),
        ),
        (
            "queue",
            cfg.sqs_endpoint_url
                .as_deref()
                .expect("local queue endpoint required"),
        ),
        ("queue URL", cfg.sqs_queue_url.as_str()),
        (
            "Cognito",
            cfg.cognito_endpoint_url
                .as_deref()
                .expect("local Cognito endpoint required"),
        ),
    ] {
        let endpoint = url::Url::parse(value).expect("test endpoint must be a URL");
        assert!(
            matches!(
                endpoint.host_str(),
                Some("localhost" | "127.0.0.1" | "[::1]" | "::1")
            ),
            "{kind} tests require a literal loopback endpoint"
        );
        if kind == "database" {
            assert_eq!(
                endpoint.port(),
                Some(5433),
                "tests require the disposable database port"
            );
            assert_eq!(
                endpoint.path(),
                "/dal_test",
                "tests require the dedicated test database"
            );
        }
    }
    assert_eq!(cfg.storage_bucket, "dal-test");
    assert!(
        cfg.sqs_queue_url
            .ends_with("/000000000000/dal-jobs-test.fifo")
    );
}

#[test]
fn isolation_rejects_remote_service_endpoints() {
    let mut cfg = test_config();
    cfg.storage_endpoint_url = Some("https://api.dal.fidan.dev".to_string());
    assert!(std::panic::catch_unwind(|| assert_isolated_config(&cfg)).is_err());
    cfg.storage_endpoint_url = Some("http://127.0.0.1:4567".to_string());
    cfg.database_url = "postgres://localhost:5433/production".to_string();
    assert!(std::panic::catch_unwind(|| assert_isolated_config(&cfg)).is_err());
}

// ── TestApp ───────────────────────────────────────────────────────────────────

pub struct TestApp {
    cognito: std::sync::Arc<dal_auth::CognitoClient>,
    pub publish_slots: std::sync::Arc<tokio::sync::Semaphore>,
    router: axum::Router,
}

impl TestApp {
    /// Builds a full `AppState` backed by real infrastructure
    /// (postgres-test + localstack-test from docker-compose.test.yml).
    pub async fn spawn() -> Self {
        // Test configuration is explicit and loopback-only; never read credential files.
        Self::with_config(test_config()).await
    }

    pub async fn with_config(cfg: Config) -> Self {
        Self::with_cognito_control_endpoint(cfg, None).await
    }

    /// Override only Cognito's control API for fault injection, keeping the real
    /// local issuer/JWKS identity intact for signed session validation.
    pub async fn with_cognito_control_endpoint(cfg: Config, endpoint: Option<&str>) -> Self {
        assert_isolated_config(&cfg);
        let mut state = AppState::build(&cfg).await.unwrap_or_else(|e| {
            panic!("TestApp::spawn failed — is docker-compose.test.yml running?\n\nError: {e:#}")
        });
        if let Some(endpoint) = endpoint {
            let mut checked = cfg.clone();
            checked.cognito_endpoint_url = Some(endpoint.to_owned());
            assert_isolated_config(&checked);
            let sdk = aws_config::defaults(aws_config::BehaviorVersion::latest())
                .region(aws_config::Region::new(cfg.aws_region.clone()))
                .credentials_provider(aws_sdk_sqs::config::Credentials::new(
                    "test",
                    "test",
                    None,
                    None,
                    "local-test",
                ))
                .retry_config(aws_config::retry::RetryConfig::disabled())
                .load()
                .await;
            state.cognito = std::sync::Arc::new(dal_auth::CognitoClient::from_sdk_config(
                &sdk,
                cfg.cognito_user_pool_id.clone(),
                cfg.cognito_client_id.clone(),
                Some(endpoint),
            ));
        }
        TestApp {
            cognito: state.cognito.clone(),
            publish_slots: state.publish_slots.clone(),
            router: build_router(state),
        }
    }

    /// Real signed JWT fixture for an existing database user, never a scope-erasing API token.
    pub async fn interactive_token(&self, db: &sqlx::PgPool, id: uuid::Uuid) -> String {
        let user = dal_db::queries::users::get_by_id(db, id)
            .await
            .unwrap()
            .unwrap();
        let password = "LocalFixturePass123!";
        let sub = self
            .cognito
            .admin_create_user(&user.username, password, &user.email)
            .await
            .unwrap();
        sqlx::query("UPDATE users SET cognito_sub=$1 WHERE id=$2")
            .bind(sub)
            .bind(id)
            .execute(db)
            .await
            .unwrap();
        let (_, jwt, _) = self
            .cognito
            .sign_in(&user.username, password)
            .await
            .unwrap();
        assert!(!jwt.is_empty());
        jwt
    }

    // ── Request helpers ───────────────────────────────────────────────────────

    pub async fn existing_interactive_token(&self, username: &str) -> String {
        self.cognito
            .sign_in(username, "LocalFixturePass123!")
            .await
            .unwrap()
            .1
    }
    pub async fn raw_request(&self, request: Request<Body>) -> Response<Body> {
        self.router.clone().oneshot(request).await.unwrap()
    }

    pub async fn get(&self, uri: &str) -> Response<Body> {
        self.router
            .clone()
            .oneshot(Request::builder().uri(uri).body(Body::empty()).unwrap())
            .await
            .unwrap()
    }

    pub async fn post_json(&self, uri: &str, body: serde_json::Value) -> Response<Body> {
        self.request_json(Method::POST, uri, body, None).await
    }

    pub async fn request_json(
        &self,
        method: Method,
        uri: &str,
        body: serde_json::Value,
        bearer_token: Option<&str>,
    ) -> Response<Body> {
        let mut builder = Request::builder()
            .method(method)
            .uri(uri)
            .header("content-type", "application/json");

        if let Some(token) = bearer_token {
            builder = builder.header("authorization", format!("Bearer {token}"));
        }

        self.router
            .clone()
            .oneshot(
                builder
                    .body(Body::from(serde_json::to_vec(&body).unwrap()))
                    .unwrap(),
            )
            .await
            .unwrap()
    }

    // ── Body helpers ──────────────────────────────────────────────────────────

    pub async fn publish_archive(&self, name: &str, token: &str, archive: &[u8]) -> Response<Body> {
        let boundary = "dal-audit-boundary";
        let mut body = format!("--{boundary}\r\nContent-Disposition: form-data; name=\"archive\"; filename=\"package.tar.gz\"\r\nContent-Type: application/gzip\r\n\r\n").into_bytes();
        body.extend_from_slice(archive);
        body.extend_from_slice(format!("\r\n--{boundary}--\r\n").as_bytes());
        self.router
            .clone()
            .oneshot(
                Request::builder()
                    .method(Method::POST)
                    .uri(format!("/packages/{name}/publish"))
                    .header("authorization", format!("Bearer {token}"))
                    .header(
                        "content-type",
                        format!("multipart/form-data; boundary={boundary}"),
                    )
                    .body(Body::from(body))
                    .unwrap(),
            )
            .await
            .unwrap()
    }

    /// Consume the response body and parse as JSON.
    pub async fn json_body(res: Response<Body>) -> serde_json::Value {
        let bytes = to_bytes(res.into_body(), usize::MAX).await.unwrap();
        serde_json::from_slice(&bytes).unwrap_or(serde_json::Value::Null)
    }

    /// Consume the response body and return the raw status + JSON.
    pub async fn unpack(res: Response<Body>) -> (StatusCode, serde_json::Value) {
        let status = res.status();
        let body = Self::json_body(res).await;
        (status, body)
    }
}

/// Returns `true` when cognito-local is configured for this test run.
/// Auth-dependant tests check this and skip themselves if false.
pub fn cognito_available() -> bool {
    std::env::var("TEST_COGNITO_ENDPOINT_URL").is_ok()
}
