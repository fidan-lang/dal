use anyhow::Context;
use aws_config::{BehaviorVersion, Region};
use dal_common::tracing_init;
use dotenvy::dotenv;
use tracing::info;

mod email;
mod jobs;
mod worker;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    dotenv().ok();
    tracing_init::init();

    let queue_url = std::env::var("DAL_SQS_QUEUE_URL").context("missing DAL_SQS_QUEUE_URL")?;
    let database_url = std::env::var("DATABASE_URL").context("missing DATABASE_URL")?;

    let db = dal_db::connect(&database_url)
        .await
        .context("connect to DB")?;

    let endpoint_url = std::env::var("DAL_SQS_ENDPOINT_URL")
        .ok()
        .filter(|s| !s.is_empty());

    log_aws_runtime_probe("dal-worker");

    let aws_cfg = aws_config::defaults(BehaviorVersion::latest())
        .region(Region::new(
            std::env::var("AWS_REGION").unwrap_or_else(|_| "eu-central-1".into()),
        ))
        .load()
        .await;
    let sqs = if let Some(ep) = endpoint_url {
        let sqs_cfg = aws_sdk_sqs::config::Builder::from(&aws_cfg)
            .endpoint_url(ep)
            .build();
        aws_sdk_sqs::Client::from_conf(sqs_cfg)
    } else {
        aws_sdk_sqs::Client::new(&aws_cfg)
    };

    let mailjet = email::MailjetClient::from_env()?;

    info!("Dal worker starting — polling {queue_url}");
    worker::run(sqs, queue_url, db, mailjet).await
}

fn log_aws_runtime_probe(component: &str) {
    info!(
        component,
        aws_region = std::env::var("AWS_REGION")
            .ok()
            .as_deref()
            .unwrap_or("<unset>"),
        aws_default_region_present = std::env::var_os("AWS_DEFAULT_REGION").is_some(),
        aws_access_key_id_present = std::env::var_os("AWS_ACCESS_KEY_ID").is_some(),
        aws_session_token_present = std::env::var_os("AWS_SESSION_TOKEN").is_some(),
        aws_container_credentials_relative_uri_present =
            std::env::var_os("AWS_CONTAINER_CREDENTIALS_RELATIVE_URI").is_some(),
        aws_container_credentials_full_uri_present =
            std::env::var_os("AWS_CONTAINER_CREDENTIALS_FULL_URI").is_some(),
        "aws runtime credential probe"
    );
}
