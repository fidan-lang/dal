use anyhow::Context;
use base64::Engine as _;
use reqwest::Client;
use serde_json::json;
use tracing::info;

/// Mailjet REST API email sender.
pub struct MailjetClient {
    http: Client,
    api_key: String,
    secret_key: String,
    from_email: String,
    from_name: String,
    base_url: String,
}

impl MailjetClient {
    pub fn from_env() -> anyhow::Result<Self> {
        Ok(Self {
            http: Client::builder()
                .timeout(std::time::Duration::from_secs(15))
                .build()
                .context("build http client")?,
            api_key: std::env::var("MAILJET_API_KEY").context("MAILJET_API_KEY")?,
            secret_key: std::env::var("MAILJET_SECRET_KEY").context("MAILJET_SECRET_KEY")?,
            from_email: std::env::var("MAILJET_FROM_EMAIL")
                .unwrap_or_else(|_| "noreply@dal.fidan.dev".into()),
            from_name: std::env::var("MAILJET_FROM_NAME")
                .unwrap_or_else(|_| "Dal Package Registry".into()),
            base_url: std::env::var("MAILJET_BASE_URL")
                .unwrap_or_else(|_| "https://api.mailjet.com/v3.1".into()),
        })
    }

    fn auth_header(&self) -> String {
        let creds = format!("{}:{}", self.api_key, self.secret_key);
        let encoded = base64::engine::general_purpose::STANDARD.encode(creds.as_bytes());
        format!("Basic {encoded}")
    }

    pub async fn send(
        &self,
        to_email: &str,
        to_name: &str,
        subject: &str,
        html_body: &str,
        text_body: &str,
    ) -> anyhow::Result<()> {
        let body = json!({
            "Messages": [{
                "From": {
                    "Email": self.from_email,
                    "Name": self.from_name,
                },
                "To": [{
                    "Email": to_email,
                    "Name": to_name,
                }],
                "Subject": subject,
                "TextPart": text_body,
                "HTMLPart": html_body,
            }]
        });

        let resp = self
            .http
            .post(format!("{}/send", self.base_url))
            .header("Authorization", self.auth_header())
            .json(&body)
            .send()
            .await
            .context("mailjet request")?;

        let status = resp.status();
        if !status.is_success() {
            // Provider response bodies may echo addresses or verification/reset links.
            anyhow::bail!("Mailjet returned {status}");
        }

        info!("email sent via Mailjet");
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    #[tokio::test]
    async fn failed_notification_is_retried_without_disclosing_provider_payload() {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let server = tokio::spawn(async move {
            let (mut socket, _) = listener.accept().await.unwrap();
            let mut buffer = [0u8; 8192];
            let _ = socket.read(&mut buffer).await.unwrap();
            socket.write_all(b"HTTP/1.1 503 Unavailable\r\nContent-Length: 21\r\nConnection: close\r\n\r\nprivate-provider-data").await.unwrap();
        });
        let mail = MailjetClient {
            http: Client::new(),
            api_key: "test".into(),
            secret_key: "test".into(),
            from_email: "test@example.test".into(),
            from_name: "Test".into(),
            base_url: format!("http://{addr}"),
        };
        let db = dal_db::PgPool::connect_lazy("postgres://dal_test:test@127.0.0.1:5433/dal_test")
            .unwrap();
        let result=crate::jobs::dispatch(&json!({"kind":"publish_notify","package":"demo","version":"1.0.0","owners":[{"email":"test@example.test","username":"test"}]}),&db,&mail).await;
        assert!(
            result.is_err(),
            "delivery failure must not acknowledge the queue message"
        );
        assert!(
            !result
                .unwrap_err()
                .to_string()
                .contains("private-provider-data")
        );
        server.await.unwrap();
    }
}
