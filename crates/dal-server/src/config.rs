use anyhow::Context;

#[derive(Debug, Clone)]
pub struct Config {
    pub database_url: String,
    pub listen_addr: String,
    pub base_url: String,
    pub storage_bucket: String,
    pub storage_endpoint_url: Option<String>,
    pub storage_access_key_id: Option<String>,
    pub storage_secret_access_key: Option<String>,
    pub storage_region: String,
    pub sqs_queue_url: String,
    pub sqs_endpoint_url: Option<String>,
    pub cognito_user_pool_id: String,
    pub cognito_client_id: String,
    pub cognito_endpoint_url: Option<String>,
    pub aws_region: String,
    pub max_upload_bytes: u64,
    pub mailjet_api_key: String,
    pub mailjet_secret_key: String,
    pub mailjet_from_email: String,
    pub mailjet_from_name: String,
}

impl Config {
    pub fn from_env() -> anyhow::Result<Self> {
        Ok(Self {
            database_url: env("DATABASE_URL")?,
            listen_addr: env_or("DAL_LISTEN_ADDR", "0.0.0.0:8080"),
            base_url: env_or("DAL_BASE_URL", "http://localhost:8080"),
            storage_bucket: env_with_fallback("DAL_STORAGE_BUCKET", "DAL_S3_BUCKET")?,
            storage_endpoint_url: env_optional_with_fallback(
                "DAL_STORAGE_ENDPOINT_URL",
                "DAL_S3_ENDPOINT_URL",
            ),
            storage_access_key_id: env_optional("DAL_STORAGE_ACCESS_KEY_ID"),
            storage_secret_access_key: env_optional("DAL_STORAGE_SECRET_ACCESS_KEY"),
            storage_region: env_or("DAL_STORAGE_REGION", "auto"),
            sqs_queue_url: env("DAL_SQS_QUEUE_URL")?,
            sqs_endpoint_url: std::env::var("DAL_SQS_ENDPOINT_URL")
                .ok()
                .filter(|s| !s.is_empty()),
            cognito_user_pool_id: env("DAL_COGNITO_USER_POOL_ID")?,
            cognito_client_id: env("DAL_COGNITO_CLIENT_ID")?,
            cognito_endpoint_url: std::env::var("DAL_COGNITO_ENDPOINT_URL")
                .ok()
                .filter(|s| !s.is_empty()),
            aws_region: env_or("AWS_REGION", "eu-central-1"),
            max_upload_bytes: std::env::var("DAL_MAX_UPLOAD_BYTES")
                .ok()
                .and_then(|v| v.parse().ok())
                .unwrap_or(52_428_800),
            mailjet_api_key: env("MAILJET_API_KEY")?,
            mailjet_secret_key: env("MAILJET_SECRET_KEY")?,
            mailjet_from_email: env_or("MAILJET_FROM_EMAIL", "noreply@dal.fidan.dev"),
            mailjet_from_name: env_or("MAILJET_FROM_NAME", "Dal Package Registry"),
        })
    }
}

fn env(key: &str) -> anyhow::Result<String> {
    std::env::var(key).with_context(|| format!("missing env var {key}"))
}

fn env_with_fallback(primary: &str, legacy: &str) -> anyhow::Result<String> {
    std::env::var(primary)
        .or_else(|_| std::env::var(legacy))
        .with_context(|| format!("missing env var {primary} (or legacy {legacy})"))
}

fn env_optional(key: &str) -> Option<String> {
    std::env::var(key).ok().filter(|s| !s.is_empty())
}

fn env_optional_with_fallback(primary: &str, legacy: &str) -> Option<String> {
    std::env::var(primary)
        .ok()
        .filter(|s| !s.is_empty())
        .or_else(|| std::env::var(legacy).ok().filter(|s| !s.is_empty()))
}

fn env_or(key: &str, default: &str) -> String {
    std::env::var(key).unwrap_or_else(|_| default.to_string())
}
