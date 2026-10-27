use std::sync::Arc;

use aws_config::{BehaviorVersion, Region};
use dal_auth::{CognitoClient, JwtValidator};
use dal_db::PgPool;
use dal_storage::StorageClient;
use tracing::info;

use crate::config::Config;
use crate::middleware::rate_limit::RateLimiter;

/// Best-effort accounting never waits for admission or retains detached DB tasks.
pub const ACCOUNTING_CONCURRENCY: usize = 8;
pub const ACCOUNTING_TIMEOUT: std::time::Duration = std::time::Duration::from_millis(250);

/// Shared application state — cheap to clone (all Arc-backed).
#[derive(Clone)]
pub struct AppState {
    pub db: PgPool,
    pub storage: Arc<StorageClient>,
    pub cognito: Arc<CognitoClient>,
    pub jwt: Arc<JwtValidator>,
    pub rate: Arc<RateLimiter>,
    pub public_reads: Arc<crate::middleware::public_reads::PublicReadLimits>,
    pub download_slots: Arc<tokio::sync::Semaphore>,
    /// SQS queue URL for dispatching background jobs.
    pub sqs_url: String,
    pub sqs_client: Arc<aws_sdk_sqs::Client>,
    pub cfg: Arc<Config>,
    /// Bound simultaneous archive buffers; keep within the provisional 512 MiB container budget.
    pub publish_slots: Arc<tokio::sync::Semaphore>,
    pub accounting_slots: Arc<tokio::sync::Semaphore>,
}

impl AppState {
    pub async fn best_effort_accounting<T>(
        &self,
        operation: impl std::future::Future<Output = Result<T, dal_common::error::DalError>>,
    ) {
        let Ok(_permit) = self.accounting_slots.try_acquire() else {
            return;
        };
        // ponytail: at most eight in-flight operations and 250ms per admitted request;
        // drop telemetry under load instead of adding an unbounded task/queue system.
        if !matches!(
            tokio::time::timeout(ACCOUNTING_TIMEOUT, operation).await,
            Ok(Ok(_))
        ) {
            tracing::debug!("best-effort accounting failed or exceeded its budget");
        }
    }

    pub async fn build(cfg: &Config) -> anyhow::Result<Self> {
        cfg.public_reads.validate()?;
        // Database
        let db = dal_db::connect(&cfg.database_url).await?;
        dal_db::migrate(&db).await?;

        log_aws_runtime_probe("dal-server");

        // Build one shared AWS SDK config for all AWS clients.
        let aws_cfg = aws_config::defaults(BehaviorVersion::latest())
            .timeout_config(
                aws_config::timeout::TimeoutConfig::builder()
                    .connect_timeout(std::time::Duration::from_secs(5))
                    .read_timeout(std::time::Duration::from_secs(30))
                    .operation_timeout(std::time::Duration::from_secs(60))
                    .build(),
            )
            .region(Region::new(cfg.aws_region.clone()))
            .load()
            .await;

        // Storage
        let storage = StorageClient::new(
            &aws_cfg,
            cfg.storage_bucket.clone(),
            cfg.storage_endpoint_url.clone(),
            cfg.storage_access_key_id.clone(),
            cfg.storage_secret_access_key.clone(),
            cfg.storage_region.clone(),
        )
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
            public_reads: Arc::new(crate::middleware::public_reads::PublicReadLimits::new(
                &cfg.public_reads,
            )),
            download_slots: Arc::new(tokio::sync::Semaphore::new(
                cfg.public_reads.concurrent_downloads,
            )),
            sqs_url: cfg.sqs_queue_url.clone(),
            sqs_client: Arc::new(sqs_client),
            cfg: Arc::new(cfg.clone()),
            publish_slots: Arc::new(tokio::sync::Semaphore::new(2)),
            accounting_slots: Arc::new(tokio::sync::Semaphore::new(ACCOUNTING_CONCURRENCY)),
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
