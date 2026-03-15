use crate::{email::MailjetClient, jobs};
use dal_db::PgPool;
use std::time::Duration;
use tokio::sync::watch;
use tracing::{error, warn};

/// Long-running SQS poll loop.
pub async fn run(
    sqs: aws_sdk_sqs::Client,
    queue_url: String,
    db: PgPool,
    mail: MailjetClient,
    mut shutdown: watch::Receiver<bool>,
) -> anyhow::Result<()> {
    loop {
        if *shutdown.borrow() {
            tracing::info!("dal-worker shutdown requested before poll — exiting");
            return Ok(());
        }

        let receive = sqs
            .receive_message()
            .queue_url(&queue_url)
            .max_number_of_messages(10)
            .wait_time_seconds(20) // long-polling
            .send();

        let resp = match tokio::select! {
            _ = shutdown.changed() => {
                tracing::info!("dal-worker shutdown requested during poll — exiting");
                return Ok(());
            }
            resp = receive => resp,
        } {
            Ok(r) => r,
            Err(e) => {
                error!(error = %e, "SQS receive_message failed — retrying in 5s");
                tokio::time::sleep(Duration::from_secs(5)).await;
                continue;
            }
        };

        let messages = resp.messages();
        if messages.is_empty() {
            continue;
        }

        for msg in messages {
            let body = msg.body().unwrap_or_default();
            let receipt = msg.receipt_handle().unwrap_or_default().to_string();

            match serde_json::from_str::<serde_json::Value>(body) {
                Ok(payload) => {
                    match jobs::dispatch(&payload, &db, &mail).await {
                        Ok(()) => {
                            // Delete the message — job succeeded
                            if let Err(e) = sqs
                                .delete_message()
                                .queue_url(&queue_url)
                                .receipt_handle(&receipt)
                                .send()
                                .await
                            {
                                warn!(error = %e, "failed to delete SQS message");
                            }
                        }
                        Err(e) => {
                            error!(error = %e, "job failed — leaving in queue for retry");
                            // Message will become visible again after visibility timeout
                        }
                    }
                }
                Err(e) => {
                    error!(error = %e, body, "malformed job payload — deleting to prevent poison pill");
                    let _ = sqs
                        .delete_message()
                        .queue_url(&queue_url)
                        .receipt_handle(&receipt)
                        .send()
                        .await;
                }
            }
        }

        if *shutdown.borrow() {
            tracing::info!("dal-worker completed current batch after shutdown request — exiting");
            return Ok(());
        }
    }
}
