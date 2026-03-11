use anyhow::Context;
use base64::Engine as _;
use reqwest::Client;
use serde_json::json;
use tracing::info;

/// Mailjet REST API email sender.
pub struct MailjetClient {
    http:       Client,
    api_key:    String,
    secret_key: String,
    from_email: String,
    from_name:  String,
    base_url:   String,
}

impl MailjetClient {
    pub fn from_env() -> anyhow::Result<Self> {
        Ok(Self {
            http: Client::builder()
                .timeout(std::time::Duration::from_secs(15))
                .build()
                .context("build http client")?,
            api_key:    std::env::var("MAILJET_API_KEY").context("MAILJET_API_KEY")?,
            secret_key: std::env::var("MAILJET_SECRET_KEY").context("MAILJET_SECRET_KEY")?,
            from_email: std::env::var("MAILJET_FROM_EMAIL")
                .unwrap_or_else(|_| "noreply@dal.fidan.dev".into()),
            from_name: std::env::var("MAILJET_FROM_NAME")
                .unwrap_or_else(|_| "Dal Package Registry".into()),
            base_url: "https://api.mailjet.com/v3.1".into(),
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
            let text = resp.text().await.unwrap_or_default();
            anyhow::bail!("Mailjet returned {status}: {text}");
        }

        info!(to = to_email, subject, "email sent via Mailjet");
        Ok(())
    }
}
