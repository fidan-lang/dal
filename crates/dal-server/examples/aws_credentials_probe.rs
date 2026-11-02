//! Read-only credential_process probe. Select AWS_PROFILE/AWS_CONFIG_FILE explicitly.
use aws_sdk_sqs::config::ProvideCredentials;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let config = aws_config::defaults(aws_config::BehaviorVersion::latest())
        .load()
        .await;
    let provider = config
        .credentials_provider()
        .ok_or_else(|| anyhow::anyhow!("credential provider unavailable"))?;
    let credentials = provider.provide_credentials().await?;
    anyhow::ensure!(
        credentials.session_token().is_some(),
        "temporary session required"
    );
    let expiry = credentials
        .expiry()
        .ok_or_else(|| anyhow::anyhow!("credential expiration required"))?;
    anyhow::ensure!(expiry > std::time::SystemTime::now(), "credentials expired");
    println!("SDK credential_process: temporary credentials loaded; expiry present");
    Ok(())
}
