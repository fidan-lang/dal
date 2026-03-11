use anyhow::Context;
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

    let queue_url = std::env::var("DAL_SQS_QUEUE_URL")
        .context("missing DAL_SQS_QUEUE_URL")?;
    let database_url = std::env::var("DATABASE_URL")
        .context("missing DATABASE_URL")?;

    let db = dal_db::connect(&database_url).await.context("connect to DB")?;

    let endpoint_url = std::env::var("DAL_SQS_ENDPOINT_URL").ok()
        .filter(|s| !s.is_empty());

    let mut aws_loader = aws_config::from_env();
    if let Some(ep) = endpoint_url {
        aws_loader = aws_loader.endpoint_url(ep);
    }
    let aws_cfg = aws_loader.load().await;
    let sqs = aws_sdk_sqs::Client::new(&aws_cfg);

    let mailjet = email::MailjetClient::from_env()?;

    info!("Dal worker starting — polling {queue_url}");
    worker::run(sqs, queue_url, db, mailjet).await
}
