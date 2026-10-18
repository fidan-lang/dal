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

        let retention_days = numeric_setting(
            std::env::var("DAL_UNVERIFIED_ACCOUNT_RETENTION_DAYS")
                .ok()
                .as_deref(),
            7_i64,
            "DAL_UNVERIFIED_ACCOUNT_RETENTION_DAYS",
        )?;
        let interval_secs = numeric_setting(
            std::env::var("DAL_ACCOUNT_CLEANUP_INTERVAL_SECS")
                .ok()
                .as_deref(),
            21_600_u64,
            "DAL_ACCOUNT_CLEANUP_INTERVAL_SECS",
        )?;
        let batch_size = numeric_setting(
            std::env::var("DAL_ACCOUNT_CLEANUP_BATCH_SIZE")
                .ok()
                .as_deref(),
            100_i64,
            "DAL_ACCOUNT_CLEANUP_BATCH_SIZE",
        )?;

        if retention_days <= 0 {
            anyhow::bail!("DAL_UNVERIFIED_ACCOUNT_RETENTION_DAYS must be > 0");
        }
        if interval_secs == 0 {
            anyhow::bail!("DAL_ACCOUNT_CLEANUP_INTERVAL_SECS must be > 0");
        }
        if batch_size <= 0 {
            anyhow::bail!("DAL_ACCOUNT_CLEANUP_BATCH_SIZE must be > 0");
        }

        if std::time::Instant::now()
            .checked_add(Duration::from_secs(interval_secs))
            .is_none()
        {
            anyhow::bail!("DAL_ACCOUNT_CLEANUP_INTERVAL_SECS is too large");
        }
        let retention = ChronoDuration::try_days(retention_days)
            .context("DAL_UNVERIFIED_ACCOUNT_RETENTION_DAYS is too large")?;
        if Utc::now().checked_sub_signed(retention).is_none() {
            anyhow::bail!("account retention is outside the supported date range");
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
            retention,
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
        interval.tick().await;

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
        let cutoff = Utc::now()
            .checked_sub_signed(self.retention)
            .context("account retention is outside the supported date range")?;
        let mut total_deleted = 0usize;

        loop {
            let users = queries::users::list_stale_unverified(&self.db, cutoff, self.batch_size)
                .await
                .context("list stale unverified users")?;

            if users.is_empty() {
                break;
            }

            let batch_len = users.len();
            let mut failed = false;
            for candidate in users {
                let mut transaction = self.db.begin().await?;
                let Some(user) =
                    queries::users::lock_stale_unverified(&mut *transaction, candidate.id, cutoff)
                        .await?
                else {
                    continue;
                };
                // Keep the row locked over provider deletion. A verification that
                // wins the lock first makes the recheck ineligible; it is never deleted.
                if self
                    .cognito
                    .admin_delete_user_if_exists(&user.username)
                    .await
                    .is_err()
                {
                    warn!(user_id = %user.id, "Cognito cleanup failed; retaining account for retry");
                    failed = true;
                    continue;
                }
                queries::users::delete_unverified(&mut *transaction, user.id).await?;
                transaction.commit().await?;
                total_deleted += 1;
                info!(user_id = %user.id, "deleted stale unverified account");
            }
            // A full failed batch must not be selected forever in this same sweep.
            if failed {
                anyhow::bail!("account cleanup deferred after provider failure");
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

fn numeric_setting<T: std::str::FromStr>(
    value: Option<&str>,
    default: T,
    name: &str,
) -> anyhow::Result<T> {
    match value {
        None => Ok(default),
        Some(value) => value
            .parse()
            .map_err(|_| anyhow::anyhow!("invalid numeric {name}")),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tokio::io::{AsyncBufReadExt, AsyncReadExt, AsyncWriteExt};
    #[test]
    fn malformed_cleanup_settings_do_not_silently_enable_defaults() {
        assert_eq!(numeric_setting(None, 7_i64, "retention").unwrap(), 7);
        assert_eq!(numeric_setting(Some("14"), 7_i64, "retention").unwrap(), 14);
        for value in ["", "seven", "999999999999999999999999999"] {
            assert!(numeric_setting(Some(value), 7_i64, "retention").is_err());
        }
    }

    #[tokio::test]
    #[ignore = "requires the disposable dal_test PostgreSQL instance; run scripts/test.sh --with-cognito"]
    async fn a_full_failed_batch_defers_cleanup_without_losing_accounts_or_spinning() {
        let database =
            std::env::var("TEST_DATABASE_URL").expect("explicit local test database required");
        assert!(database.ends_with("@127.0.0.1:5433/dal_test"));
        let db = dal_db::connect(&database).await.unwrap();
        dal_db::migrate(&db).await.unwrap();
        let id = uuid::Uuid::new_v4();
        let name = format!("cleanup-{}", id.simple());
        queries::users::create(
            &db,
            id,
            &name,
            &format!("{name}@example.test"),
            &id.to_string(),
            None,
        )
        .await
        .unwrap();
        sqlx::query("UPDATE users SET created_at = now() - interval '30 days' WHERE id = $1")
            .bind(id)
            .execute(&db)
            .await
            .unwrap();
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let endpoint = format!("http://{}", listener.local_addr().unwrap());
        let calls = std::sync::Arc::new(std::sync::atomic::AtomicUsize::new(0));
        let observed = calls.clone();
        let server = tokio::spawn(async move {
            loop {
                let (socket, _) = listener.accept().await.unwrap();
                let mut socket = tokio::io::BufReader::new(socket);
                let mut content_length = None;
                let mut header_bytes = 0;
                loop {
                    let mut line = String::new();
                    let read = socket.read_line(&mut line).await.unwrap();
                    assert!(read > 0);
                    header_bytes += read;
                    assert!(header_bytes <= 16384);
                    if line == "\r\n" {
                        break;
                    }
                    if let Some((name, value)) = line.split_once(':')
                        && name.eq_ignore_ascii_case("content-length")
                    {
                        content_length = Some(value.trim().parse::<usize>().unwrap());
                    }
                }
                let length = content_length.expect("SDK JSON request has a content length");
                assert!(length <= 8192);
                socket.read_exact(&mut vec![0; length]).await.unwrap();
                observed.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
                let body =
                    r#"{"__type":"NotAuthorizedException","message":"local injected failure"}"#;
                let response = format!(
                    "HTTP/1.1 400 Bad Request\r\nContent-Type: application/x-amz-json-1.1\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                    body.len()
                );
                socket
                    .get_mut()
                    .write_all(response.as_bytes())
                    .await
                    .unwrap();
            }
        });
        let config = aws_config::defaults(aws_config::BehaviorVersion::latest())
            .region(aws_config::Region::new("eu-central-1"))
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
        let cleaner = StaleAccountCleaner {
            db: db.clone(),
            cognito: CognitoClient::from_sdk_config(
                &config,
                "local_test".into(),
                "test-client".into(),
                Some(&endpoint),
            ),
            retention: ChronoDuration::days(7),
            interval: Duration::from_secs(3600),
            batch_size: 1,
        };
        let result = tokio::time::timeout(Duration::from_secs(3), cleaner.cleanup_once())
            .await
            .expect("failed full batch must terminate");
        assert!(result.is_err());
        assert_eq!(calls.load(std::sync::atomic::Ordering::Relaxed), 1);
        assert!(queries::users::get_by_id(&db, id).await.unwrap().is_some());
        queries::users::delete_unverified(&db, id).await.unwrap();
        server.abort();
    }
}
