use crate::email::MailjetClient;
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

async fn handle_email_verify(payload: &Value, mail: &MailjetClient) -> anyhow::Result<()> {
    let email = payload["email"].as_str().context("email")?;
    let username = payload["username"].as_str().context("username")?;
    let token = payload["token"].as_str().context("token")?;

    let base_url = std::env::var("DAL_BASE_URL").unwrap_or_else(|_| "https://dal.fidan.dev".into());
    let link = format!("{base_url}/verify-email?token={token}");

    let subject = "Verify your Dal account".to_string();
    let text_body = format!(
        "Hi {username},\n\nVerify your email address:\n{link}\n\nThis link expires in 24 hours.\n\nThe Dal team"
    );
    let html_body = format!(
        r#"<p>Hi <strong>{username}</strong>,</p>
           <p>Verify your email address to activate your Dal account:</p>
           <p><a href="{link}" style="background:#7c6dff;color:#fff;padding:10px 20px;border-radius:6px;text-decoration:none">Verify Email</a></p>
           <p>This link expires in 24 hours.</p>
           <p>The Dal team</p>"#
    );

    mail.send(email, username, &subject, &html_body, &text_body)
        .await
}

async fn handle_password_reset(payload: &Value, mail: &MailjetClient) -> anyhow::Result<()> {
    let email = payload["email"].as_str().context("email")?;
    let username = payload["username"].as_str().context("username")?;
    let token = payload["token"].as_str().context("token")?;

    let base_url = std::env::var("DAL_BASE_URL").unwrap_or_else(|_| "https://dal.fidan.dev".into());
    let link = format!("{base_url}/reset-password?token={token}");

    let subject = "Reset your Dal password".to_string();
    let text_body = format!(
        "Hi {username},\n\nReset your password:\n{link}\n\nThis link expires in 1 hour.\n\nIf you didn't request this, ignore this email.\n\nThe Dal team"
    );
    let html_body = format!(
        r#"<p>Hi <strong>{username}</strong>,</p>
           <p>Click the button below to reset your password:</p>
           <p><a href="{link}" style="background:#7c6dff;color:#fff;padding:10px 20px;border-radius:6px;text-decoration:none">Reset Password</a></p>
           <p>This link expires in 1 hour. If you didn't request this, ignore this email.</p>
           <p>The Dal team</p>"#
    );

    mail.send(email, username, &subject, &html_body, &text_body)
        .await
}

async fn handle_owner_invite(payload: &Value, mail: &MailjetClient) -> anyhow::Result<()> {
    let email = payload["email"].as_str().context("email")?;
    let username = payload["username"].as_str().context("username")?;
    let inviter_username = payload["inviter_username"]
        .as_str()
        .context("inviter_username")?;
    let package = payload["package"].as_str().context("package")?;
    let role = payload["role"].as_str().unwrap_or("collaborator");

    let base_url = std::env::var("DAL_BASE_URL").unwrap_or_else(|_| "https://dal.fidan.dev".into());
    let link = format!("{base_url}/dashboard");

    let subject = format!("You're invited to collaborate on {package}");
    let text_body = format!(
        "Hi {username},\n\n{inviter_username} invited you to join {package} as a {role}.\n\nReview the invite from your dashboard:\n{link}\n\nThe Dal team"
    );
    let html_body = format!(
        r#"<p>Hi <strong>{username}</strong>,</p>
           <p><strong>{inviter_username}</strong> invited you to join <strong>{package}</strong> as a <strong>{role}</strong>.</p>
           <p><a href="{link}" style="background:#7c6dff;color:#fff;padding:10px 20px;border-radius:6px;text-decoration:none">Open Dashboard</a></p>
           <p>The Dal team</p>"#
    );

    mail.send(email, username, &subject, &html_body, &text_body)
        .await
}

async fn handle_publish_notify(payload: &Value, mail: &MailjetClient) -> anyhow::Result<()> {
    // Notify all owners of a package that a new version was published
    let package = payload["package"].as_str().context("package")?;
    let version = payload["version"].as_str().context("version")?;
    let publisher = payload["publisher"].as_str().unwrap_or("unknown");

    if let Some(owners) = payload["owners"].as_array() {
        for owner in owners {
            let email = owner["email"].as_str().unwrap_or_default();
            let username = owner["username"].as_str().unwrap_or_default();
            let base_url =
                std::env::var("DAL_BASE_URL").unwrap_or_else(|_| "https://dal.fidan.dev".into());
            let link = format!("{base_url}/packages/{package}");

            let subject = format!("{package} v{version} published");
            let text_body = format!(
                "Hi {username},\n\n{publisher} published {package} v{version}.\n\nView it at {link}\n\nThe Dal team"
            );
            let html_body = format!(
                r#"<p>Hi <strong>{username}</strong>,</p>
                   <p><strong>{publisher}</strong> published <strong>{package} v{version}</strong>.</p>
                   <p><a href="{link}">View on Dal</a></p>
                   <p>The Dal team</p>"#
            );

            if let Err(e) = mail
                .send(email, username, &subject, &html_body, &text_body)
                .await
            {
                warn!(error = %e, to = email, "failed to send publish notification");
            }
        }
    }

    Ok(())
}
