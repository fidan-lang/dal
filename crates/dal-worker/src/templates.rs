//! Shared transactional email presentation. All user-controlled values are HTML-escaped
//! before interpolation. The logo is delivered as an inline CID image by MailjetClient.

pub struct EmailMessage {
    pub subject: String,
    pub text: String,
    pub html: String,
}

fn escape_html(value: &str) -> String {
    let mut escaped = String::with_capacity(value.len());
    for c in value.chars() {
        match c {
            '&' => escaped.push_str("&amp;"),
            '<' => escaped.push_str("&lt;"),
            '>' => escaped.push_str("&gt;"),
            '"' => escaped.push_str("&quot;"),
            '\'' => escaped.push_str("&#39;"),
            _ => escaped.push(c),
        }
    }
    escaped
}

fn render(
    username: &str,
    preheader: &str,
    eyebrow: &str,
    title: &str,
    paragraph_html: &str,
    button_label: &str,
    link: &str,
    note: &str,
) -> String {
    let username = escape_html(username);
    let preheader = escape_html(preheader);
    let eyebrow = escape_html(eyebrow);
    let title = escape_html(title);
    let button_label = escape_html(button_label);
    let link = escape_html(link);
    let note = escape_html(note);
    format!(
        r##"<!doctype html>
<html lang="en"><head><meta charset="utf-8"><meta name="viewport" content="width=device-width,initial-scale=1"><title>Dal</title></head>
<body style="margin:0;padding:0;background:#080f08;color:#e8f0e8;font-family:Arial,Helvetica,sans-serif;-webkit-text-size-adjust:100%;">
<div style="display:none!important;font-size:1px;color:#080f08;line-height:1px;max-height:0;max-width:0;opacity:0;overflow:hidden;">{preheader}</div>
<table role="presentation" width="100%" cellpadding="0" cellspacing="0" border="0" style="background:#080f08;border-collapse:collapse;"><tr><td align="center" style="padding:36px 14px;">
<table role="presentation" width="100%" cellpadding="0" cellspacing="0" border="0" style="max-width:584px;background:#111a11;border:1px solid #304430;border-collapse:separate;border-spacing:0;border-radius:16px;overflow:hidden;">
<tr><td style="padding:25px 34px;background:#232b22;border-bottom:1px solid #304430;">
<table role="presentation" cellpadding="0" cellspacing="0" border="0" width="100%"><tr>
<td align="left" valign="middle" style="white-space:nowrap;"><img src="cid:dal-logo" width="50" height="32" alt="" style="display:inline-block;vertical-align:middle;border:0;outline:0;width:50px;height:32px;" /> <span style="display:inline-block;vertical-align:middle;color:#e8f0e8;font-size:27px;line-height:32px;font-weight:bold;letter-spacing:-1px;">dal</span></td>
<td align="right" valign="middle" style="color:#a2c18b;font-size:10px;line-height:14px;font-weight:bold;letter-spacing:1px;">FIDAN PACKAGE REGISTRY</td>
</tr></table></td></tr>
<tr><td style="padding:38px 34px 34px;">
<p style="margin:0 0 12px;color:#a3dc67;font-size:12px;font-weight:bold;letter-spacing:1.3px;">{eyebrow}</p>
<h1 style="margin:0 0 22px;color:#f3f8f2;font-size:29px;line-height:37px;font-weight:bold;letter-spacing:-0.7px;">{title}</h1>
<p style="margin:0 0 15px;color:#d7e6d7;font-size:16px;line-height:25px;">Hi <strong>{username}</strong>,</p>
<p style="margin:0 0 27px;color:#c5d6c4;font-size:15px;line-height:25px;">{paragraph_html}</p>
<table role="presentation" cellpadding="0" cellspacing="0" border="0"><tr><td align="center" bgcolor="#84cc16" style="background:#84cc16;border-radius:10px;"><a href="{link}" style="display:inline-block;padding:15px 24px;border:1px solid #84cc16;border-radius:10px;background:#84cc16;color:#142408;text-decoration:none;font-size:15px;line-height:20px;font-weight:bold;">{button_label}</a></td></tr></table>
<p style="margin:24px 0 0;color:#97ad97;font-size:13px;line-height:21px;">{note}</p>
<table role="presentation" cellpadding="0" cellspacing="0" border="0" width="100%" style="margin-top:28px;border-top:1px solid #304430;"><tr><td style="padding-top:19px;"><p style="margin:0 0 9px;color:#97ad97;font-size:12px;line-height:19px;">Button not working? Copy and paste this URL:</p><p style="margin:0;overflow-wrap:anywhere;word-break:break-all;font-size:12px;line-height:19px;"><a href="{link}" style="color:#bef264;text-decoration:underline;">{link}</a></p></td></tr></table>
</td></tr>
<tr><td style="padding:20px 34px 24px;background:#0c150c;border-top:1px solid #304430;"><p style="margin:0;color:#a0b59f;font-size:12px;line-height:19px;">Dal is the package registry for the Fidan programming language.</p><p style="margin:7px 0 0;font-size:12px;"><a href="https://dal.fidan.dev" style="color:#bef264;text-decoration:none;">dal.fidan.dev</a></p></td></tr>
</table><p style="margin:18px 0 0;color:#829b82;font-size:11px;line-height:18px;">Transactional message from Dal. No marketing subscription required.</p>
</td></tr></table></body></html>"##
    )
}

pub fn verify(username: &str, link: &str) -> EmailMessage {
    EmailMessage {
        subject: "Verify your Dal account".into(),
        text: format!(
            "Hi {username},\n\nVerify your email address to activate your Dal account:\n{link}\n\nThis link expires in 24 hours. If you did not create an account, ignore this email.\n\nThe Dal team"
        ),
        html: render(
            username,
            "Activate your Dal account with one click.",
            "WELCOME TO DAL",
            "Verify your email address",
            "Confirm your email address to activate your account and start publishing packages for Fidan.",
            "Verify email address",
            link,
            "This link expires in 24 hours. Didn't create an account? You can safely ignore this message.",
        ),
    }
}

pub fn reset(username: &str, link: &str) -> EmailMessage {
    EmailMessage {
        subject: "Reset your Dal password".into(),
        text: format!(
            "Hi {username},\n\nReset your Dal password:\n{link}\n\nThis link expires in 1 hour. If you didn't request this, ignore this email.\n\nThe Dal team"
        ),
        html: render(
            username,
            "Securely reset your Dal account password.",
            "ACCOUNT SECURITY",
            "Reset your password",
            "We received a request to reset your Dal password. Use the button below to choose a new one.",
            "Reset password",
            link,
            "This link expires in 1 hour. If you didn't request a reset, you can safely ignore this message.",
        ),
    }
}

pub fn invite(
    username: &str,
    inviter: &str,
    package: &str,
    role: &str,
    link: &str,
) -> EmailMessage {
    let paragraph = format!(
        "<strong>{}</strong> invited you to collaborate on <strong>{}</strong> as a <strong>{}</strong>. Review the invitation in your Dal dashboard.",
        escape_html(inviter),
        escape_html(package),
        escape_html(role)
    );
    EmailMessage {
        subject: format!("You're invited to collaborate on {package}"),
        text: format!(
            "Hi {username},\n\n{inviter} invited you to join {package} as a {role}.\n\nReview the invitation: {link}\n\nThe Dal team"
        ),
        html: render(
            username,
            "You've been invited to collaborate on Dal.",
            "PACKAGE COLLABORATION",
            "You've been invited",
            &paragraph,
            "Review invitation",
            link,
            "Only accept invitations from people you recognize.",
        ),
    }
}

pub fn published(
    username: &str,
    publisher: &str,
    package: &str,
    version: &str,
    link: &str,
) -> EmailMessage {
    let paragraph = format!(
        "<strong>{}</strong> published <strong>{} v{}</strong>. You can view the new release in Dal.",
        escape_html(publisher),
        escape_html(package),
        escape_html(version)
    );
    EmailMessage {
        subject: format!("{package} v{version} published"),
        text: format!(
            "Hi {username},\n\n{publisher} published {package} v{version}.\n\nView the package: {link}\n\nThe Dal team"
        ),
        html: render(
            username,
            "A new package version is available.",
            "PACKAGE ACTIVITY",
            "New version published",
            &paragraph,
            "View package",
            link,
            "This notification was triggered by activity on a package you own.",
        ),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn all_transactional_emails_are_branded_and_have_plaintext_fallback() {
        let link = "https://dal.fidan.dev/verify-email?token=fake_test_only";
        for msg in [
            verify("test", link),
            reset("test", link),
            invite("test", "alice", "demo", "owner", link),
            published("test", "alice", "demo", "1.0", link),
        ] {
            assert!(msg.html.contains("cid:dal-logo"));
            assert!(msg.html.contains("#84cc16"));
            assert!(msg.html.contains("FIDAN PACKAGE REGISTRY"));
            assert!(msg.html.contains(link));
            assert!(msg.text.contains(link));
        }
    }

    #[test]
    fn untrusted_names_and_links_are_html_escaped() {
        let msg = invite(
            "<script>",
            "<img>",
            "\" onmouseover=\"x",
            "<&",
            "https://dal.fidan.dev/?a=1&b=2",
        );
        assert!(!msg.html.contains("<script>"));
        assert!(!msg.html.contains("<img>"));
        assert!(!msg.html.contains("onmouseover=\"x"));
        assert!(msg.html.contains("&lt;script&gt;"));
        assert!(msg.html.contains("?a=1&amp;b=2"));
    }
}
