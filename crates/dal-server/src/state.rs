use std::sync::Arc;

use aws_config::{BehaviorVersion, Region};
use dal_auth::{CognitoClient, JwtValidator};
use dal_db::PgPool;
use dal_storage::StorageClient;
use tracing::info;

use crate::config::Config;
use crate::middleware::rate_limit::RateLimiter;

/// Shared application state — cheap to clone (all Arc-backed).
#[derive(Clone)]
pub struct AppState {
    pub db: PgPool,
    pub storage: Arc<StorageClient>,
    pub cognito: Arc<CognitoClient>,
    pub jwt: Arc<JwtValidator>,
    pub rate: Arc<RateLimiter>,
    /// SQS queue URL for dispatching background jobs.
    pub sqs_url: String,
    pub sqs_client: Arc<aws_sdk_sqs::Client>,
    pub cfg: Arc<Config>,
}

impl AppState {
    pub async fn build(cfg: &Config) -> anyhow::Result<Self> {
        // Database
        let db = dal_db::connect(&cfg.database_url).await?;
        dal_db::migrate(&db).await?;

        log_aws_runtime_probe("dal-server");

        // Build one shared AWS SDK config for all AWS clients.
        let aws_cfg = aws_config::defaults(BehaviorVersion::latest())
            .region(Region::new(cfg.aws_region.clone()))
            .load()
            .await;

        // Storage
        let storage =
            StorageClient::new(&aws_cfg, cfg.s3_bucket.clone(), cfg.s3_endpoint_url.clone())
                .await?;

        // Cognito
        let cognito = CognitoClient::from_sdk_config(
            &aws_cfg,
            cfg.cognito_user_pool_id.clone(),
            cfg.cognito_client_id.clone(),
            cfg.cognito_endpoint_url.as_deref(),
        );

        // JWT validator (JWKS fetched lazily on first request)
        let jwt = JwtValidator::new(
            &cfg.aws_region,
            &cfg.cognito_user_pool_id,
            &cfg.cognito_client_id,
            cfg.cognito_endpoint_url.as_deref(),
        );

        // SQS — optional endpoint override for LocalStack
        let sqs_client = if let Some(ref ep) = cfg.sqs_endpoint_url {
            let sqs_cfg = aws_sdk_sqs::config::Builder::from(&aws_cfg)
                .endpoint_url(ep)
                .build();
            aws_sdk_sqs::Client::from_conf(sqs_cfg)
        } else {
            aws_sdk_sqs::Client::new(&aws_cfg)
        };

        // Rate limiter
        let rate = RateLimiter::new();

        Ok(Self {
            db,
            storage: Arc::new(storage),
            cognito: Arc::new(cognito),
            jwt: Arc::new(jwt),
            rate: Arc::new(rate),
            sqs_url: cfg.sqs_queue_url.clone(),
            sqs_client: Arc::new(sqs_client),
            cfg: Arc::new(cfg.clone()),
        })
    }
}

fn log_aws_runtime_probe(component: &str) {
    info!(
        component,
        aws_region = std::env::var("AWS_REGION")
            .ok()
            .as_deref()
            .unwrap_or("<unset>"),
        aws_default_region_present = env_present("AWS_DEFAULT_REGION"),
        aws_access_key_id_present = env_present("AWS_ACCESS_KEY_ID"),
        aws_session_token_present = env_present("AWS_SESSION_TOKEN"),
        aws_container_credentials_relative_uri_present =
            env_present("AWS_CONTAINER_CREDENTIALS_RELATIVE_URI"),
        aws_container_credentials_full_uri_present =
            env_present("AWS_CONTAINER_CREDENTIALS_FULL_URI"),
        "aws runtime credential probe"
    );
}

fn env_present(key: &str) -> bool {
    std::env::var_os(key).is_some()
}
