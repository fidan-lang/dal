use std::time::Duration;

use anyhow::Context;
use chrono::{Duration as ChronoDuration, Utc};
use dal_auth::CognitoClient;
use dal_db::{PgPool, queries};
use tokio::{sync::watch, time::MissedTickBehavior};
use tracing::{error, info, warn};

pub struct StaleAccountCleaner {
    db: PgPool,
    cognito: CognitoClient,
    retention: ChronoDuration,
    interval: Duration,
    batch_size: i64,
}

impl StaleAccountCleaner {
    pub fn from_env(db: PgPool, aws_cfg: &aws_config::SdkConfig) -> anyhow::Result<Option<Self>> {
        let pool_id = match std::env::var("DAL_COGNITO_USER_POOL_ID") {
            Ok(value) if !value.trim().is_empty() => value,
            _ => return Ok(None),
        };
        let client_id = match std::env::var("DAL_COGNITO_CLIENT_ID") {
            Ok(value) if !value.trim().is_empty() => value,
            _ => return Ok(None),
        };

        let retention_days = std::env::var("DAL_UNVERIFIED_ACCOUNT_RETENTION_DAYS")
            .ok()
            .and_then(|value| value.parse::<i64>().ok())
            .unwrap_or(7);
        let interval_secs = std::env::var("DAL_ACCOUNT_CLEANUP_INTERVAL_SECS")
            .ok()
            .and_then(|value| value.parse::<u64>().ok())
            .unwrap_or(21_600);
        let batch_size = std::env::var("DAL_ACCOUNT_CLEANUP_BATCH_SIZE")
            .ok()
            .and_then(|value| value.parse::<i64>().ok())
            .unwrap_or(100);

        if retention_days <= 0 {
            anyhow::bail!("DAL_UNVERIFIED_ACCOUNT_RETENTION_DAYS must be > 0");
        }
        if interval_secs == 0 {
            anyhow::bail!("DAL_ACCOUNT_CLEANUP_INTERVAL_SECS must be > 0");
        }
        if batch_size <= 0 {
            anyhow::bail!("DAL_ACCOUNT_CLEANUP_BATCH_SIZE must be > 0");
        }

        let cognito = CognitoClient::from_sdk_config(
            aws_cfg,
            pool_id,
            client_id,
            std::env::var("DAL_COGNITO_ENDPOINT_URL")
                .ok()
                .filter(|value| !value.is_empty())
                .as_deref(),
        );

        Ok(Some(Self {
            db,
            cognito,
            retention: ChronoDuration::days(retention_days),
            interval: Duration::from_secs(interval_secs),
            batch_size,
        }))
    }

    pub async fn run(self, mut shutdown: watch::Receiver<bool>) -> anyhow::Result<()> {
        info!(
            retention_days = self.retention.num_days(),
            interval_secs = self.interval.as_secs(),
            batch_size = self.batch_size,
            "stale unverified account cleanup enabled"
        );

        if let Err(err) = self.cleanup_once().await {
            error!(error = %err, "initial stale unverified account cleanup failed");
        }

        let mut interval = tokio::time::interval(self.interval);
        interval.set_missed_tick_behavior(MissedTickBehavior::Skip);

        loop {
            tokio::select! {
                _ = shutdown.changed() => {
                    info!("stale unverified account cleanup stopping due to shutdown");
                    return Ok(());
                }
                _ = interval.tick() => {
                    if let Err(err) = self.cleanup_once().await {
                        error!(error = %err, "stale unverified account cleanup failed");
                    }
                }
            }
        }
    }

    async fn cleanup_once(&self) -> anyhow::Result<()> {
        let cutoff = Utc::now() - self.retention;
        let mut total_deleted = 0usize;

        loop {
            let users = queries::users::list_stale_unverified(&self.db, cutoff, self.batch_size)
                .await
                .context("list stale unverified users")?;

            if users.is_empty() {
                break;
            }

            let batch_len = users.len();
            for user in users {
                match self
                    .cognito
                    .admin_delete_user_if_exists(&user.username)
                    .await
                {
                    Ok(_) => {}
                    Err(err) => {
                        warn!(
                            username = %user.username,
                            user_id = %user.id,
                            error = %err,
                            "failed to delete stale unverified user from Cognito; leaving DB row for retry"
                        );
                        continue;
                    }
                }

                match queries::users::delete_unverified(&self.db, user.id).await {
                    Ok(true) => {
                        total_deleted += 1;
                        info!(
                            username = %user.username,
                            user_id = %user.id,
                            "deleted stale unverified account"
                        );
                    }
                    Ok(false) => {
                        info!(
                            username = %user.username,
                            user_id = %user.id,
                            "stale account was verified or removed before cleanup completed"
                        );
                    }
                    Err(err) => {
                        warn!(
                            username = %user.username,
                            user_id = %user.id,
                            error = %err,
                            "failed to delete stale unverified user from database after Cognito cleanup"
                        );
                    }
                }
            }

            if batch_len < self.batch_size as usize {
                break;
            }
        }

        if total_deleted > 0 {
            info!(
                deleted = total_deleted,
                "stale unverified account cleanup completed"
            );
        }

        Ok(())
    }
}
