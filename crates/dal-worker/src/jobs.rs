use crate::{email::MailjetClient, templates};
use anyhow::Context;
use dal_db::PgPool;
use serde_json::Value;
use tracing::warn;

/// Dispatch a single job payload to the appropriate handler.
pub async fn dispatch(payload: &Value, _db: &PgPool, mail: &MailjetClient) -> anyhow::Result<()> {
    let kind = payload["kind"].as_str().unwrap_or("unknown");
    match kind {
        "email_verify" => handle_email_verify(payload, mail).await,
        "password_reset" => handle_password_reset(payload, mail).await,
        "owner_invite" => handle_owner_invite(payload, mail).await,
        "publish_notify" => handle_publish_notify(payload, mail).await,
        other => {
            warn!(kind = other, "unknown job kind — skipping");
            Ok(())
        }
    }
}

fn base_url() -> String {
    std::env::var("DAL_BASE_URL").unwrap_or_else(|_| "https://dal.fidan.dev".into())
}

async fn handle_email_verify(payload: &Value, mail: &MailjetClient) -> anyhow::Result<()> {
    let email = payload["email"].as_str().context("email")?;
    let username = payload["username"].as_str().context("username")?;
    let token = payload["token"].as_str().context("token")?;
    let link = format!("{}/verify-email?token={token}", base_url());
    let message = templates::verify(username, &link);
    mail.send(
        email,
        username,
        &message.subject,
        &message.html,
        &message.text,
    )
    .await
}

async fn handle_password_reset(payload: &Value, mail: &MailjetClient) -> anyhow::Result<()> {
    let email = payload["email"].as_str().context("email")?;
    let username = payload["username"].as_str().context("username")?;
    let token = payload["token"].as_str().context("token")?;
    let link = format!("{}/reset-password?token={token}", base_url());
    let message = templates::reset(username, &link);
    mail.send(
        email,
        username,
        &message.subject,
        &message.html,
        &message.text,
    )
    .await
}

async fn handle_owner_invite(payload: &Value, mail: &MailjetClient) -> anyhow::Result<()> {
    let email = payload["email"].as_str().context("email")?;
    let username = payload["username"].as_str().context("username")?;
    let inviter = payload["inviter_username"]
        .as_str()
        .context("inviter_username")?;
    let package = payload["package"].as_str().context("package")?;
    let role = payload["role"].as_str().unwrap_or("collaborator");
    let link = format!("{}/dashboard", base_url());
    let message = templates::invite(username, inviter, package, role, &link);
    mail.send(
        email,
        username,
        &message.subject,
        &message.html,
        &message.text,
    )
    .await
}

async fn handle_publish_notify(payload: &Value, mail: &MailjetClient) -> anyhow::Result<()> {
    let package = payload["package"].as_str().context("package")?;
    let version = payload["version"].as_str().context("version")?;
    let publisher = payload["publisher"].as_str().unwrap_or("unknown");
    let link = format!("{}/packages/{package}", base_url());

    let mut failures = 0;
    if let Some(owners) = payload["owners"].as_array() {
        for owner in owners {
            let email = owner["email"].as_str().unwrap_or_default();
            let username = owner["username"].as_str().unwrap_or_default();
            let message = templates::published(username, publisher, package, version, &link);
            if let Err(e) = mail
                .send(
                    email,
                    username,
                    &message.subject,
                    &message.html,
                    &message.text,
                )
                .await
            {
                failures += 1;
                warn!(error = %e, "failed to send publish notification");
            }
        }
    }
    if failures > 0 {
        anyhow::bail!("{failures} publication notifications failed; queue retry required");
    }
    Ok(())
}
