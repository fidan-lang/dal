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

fn test_config() -> Config {
    Config {
        database_url: std::env::var("TEST_DATABASE_URL")
            .unwrap_or_else(|_| "postgres://dal_test:test@localhost:5433/dal_test".to_string()),
        listen_addr: "127.0.0.1:0".to_string(),
        base_url: "http://localhost".to_string(),
        s3_bucket: "dal-test".to_string(),
        s3_endpoint_url: Some(
            std::env::var("TEST_S3_ENDPOINT_URL")
                .unwrap_or_else(|_| "http://localhost:4567".to_string()),
        ),
        sqs_queue_url: std::env::var("TEST_SQS_QUEUE_URL").unwrap_or_else(|_| {
            "http://sqs.eu-central-1.localhost.localstack.cloud:4567/000000000000/dal-jobs-test.fifo"
                .to_string()
        }),
        sqs_endpoint_url: Some(
            std::env::var("TEST_SQS_ENDPOINT_URL")
                .unwrap_or_else(|_| "http://localhost:4567".to_string()),
        ),
        cognito_user_pool_id: std::env::var("TEST_COGNITO_POOL_ID")
            .unwrap_or_else(|_| "local_0dPm2L0N".to_string()),
        cognito_client_id: std::env::var("TEST_COGNITO_CLIENT_ID")
            .unwrap_or_else(|_| "bs5obcfdxmvh7g6ldnqmeahae".to_string()),
        cognito_endpoint_url: std::env::var("TEST_COGNITO_ENDPOINT_URL").ok(),
        aws_region: std::env::var("AWS_REGION")
            .unwrap_or_else(|_| "eu-central-1".to_string()),
        max_upload_bytes: 10_485_760,
        mailjet_api_key: "test_key".to_string(),
        mailjet_secret_key: "test_secret".to_string(),
        mailjet_from_email: "test@dal.test".to_string(),
        mailjet_from_name: "Dal Tests".to_string(),
    }
}

// ── TestApp ───────────────────────────────────────────────────────────────────

pub struct TestApp {
    router: axum::Router,
}

impl TestApp {
    /// Builds a full `AppState` backed by real infrastructure
    /// (postgres-test + localstack-test from docker-compose.test.yml).
    pub async fn spawn() -> Self {
        // Load .env.test if present (silently skip if missing)
        dotenvy::from_filename(".env.test").ok();

        let cfg = test_config();
        let state = AppState::build(&cfg).await.unwrap_or_else(|e| {
            panic!("TestApp::spawn failed — is docker-compose.test.yml running?\n\nError: {e:#}")
        });
        TestApp {
            router: build_router(state),
        }
    }

    // ── Request helpers ───────────────────────────────────────────────────────

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
