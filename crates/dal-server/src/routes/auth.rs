use axum::{
    Json, Router,
    body::Bytes,
    extract::State,
    http::{HeaderMap, StatusCode, header},
    routing::{get, post},
};
use chrono::{Duration, Utc};
use serde::Deserialize;
use serde_json::{Value, json};
use url::Url;
use uuid::Uuid;

use dal_auth::api_token::hash_token as sha256_hex;
use dal_common::error::DalError;
use dal_db::queries;

use crate::{extractors::AuthUser, state::AppState};

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/auth/register", post(register))
        .route("/auth/resend-verification", post(resend_verification))
        .route("/auth/login", post(login))
        .route("/auth/logout", post(logout))
        .route("/auth/refresh", post(refresh))
        .route("/auth/verify-email", get(verify_email))
        .route("/auth/forgot-password", post(forgot_password))
        .route("/auth/reset-password", post(reset_password))
        .route("/auth/me", get(me))
}

const ACCESS_COOKIE_NAME: &str = "dal_access_token";
const REFRESH_COOKIE_NAME: &str = "dal_refresh_token";

fn cookie_attrs(path: &str, max_age: i64, base_url: &str) -> String {
    let secure = if base_url.trim().starts_with("https://") {
        "; Secure"
    } else {
        ""
    };
    let domain = cookie_domain_attr(base_url);

    format!("{secure}{domain}; HttpOnly; SameSite=Strict; Path={path}; Max-Age={max_age}")
}

fn cookie_domain_attr(base_url: &str) -> String {
    let Ok(url) = Url::parse(base_url.trim()) else {
        return String::new();
    };

    let Some(host) = url.host_str() else {
        return String::new();
    };

    if host.eq_ignore_ascii_case("localhost") || host.parse::<std::net::IpAddr>().is_ok() {
        return String::new();
    }

    format!("; Domain={host}")
}

// ── Register ─────────────────────────────────────────────────────────────────

#[derive(Deserialize)]
struct RegisterBody {
    username: String,
    email: String,
    password: String,
    display_name: Option<String>,
}

async fn register(
    State(state): State<AppState>,
    Json(body): Json<RegisterBody>,
) -> Result<(StatusCode, Json<Value>), DalError> {
    // Input validation
    let username = body.username.trim().to_lowercase();
    let email = body.email.trim().to_lowercase();

    if username.len() < 3 || username.len() > 32 {
        return Err(DalError::Validation("username must be 3–32 chars".into()));
    }
    if !username
        .chars()
        .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
    {
        return Err(DalError::Validation(
            "username may only contain letters, digits, hyphens and underscores".into(),
        ));
    }
    if body.password.len() < 8 {
        return Err(DalError::Validation(
            "password must be at least 8 chars".into(),
        ));
    }

    // Uniqueness checks
    if queries::users::username_exists(&state.db, &username).await? {
        return Err(DalError::UsernameTaken(username));
    }
    if queries::users::email_exists(&state.db, &email).await? {
        return Err(DalError::EmailTaken);
    }

    // Create Cognito user (admin API — no Cognito email sending, user is CONFIRMED immediately)
    let cognito_sub = state
        .cognito
        .admin_create_user(&username, &body.password, &email)
        .await?;

    // Create DB user
    let user_id = Uuid::new_v4();
    if let Err(err) = queries::users::create(
        &state.db,
        user_id,
        &username,
        &email,
        &cognito_sub,
        body.display_name.as_deref(),
    )
    .await
    {
        if let Err(cleanup_err) = state.cognito.admin_delete_user_if_exists(&username).await {
            tracing::error!(
                username,
                error = %cleanup_err,
                "failed to roll back Cognito user after registration DB failure"
            );
        }
        return Err(err.into());
    }

    // Generate + store email verification token
    let raw_token = format!("ev_{}", hex::encode(Uuid::new_v4().as_bytes()));
    let token_hash = sha256_hex(&raw_token);
    let expires_at = Utc::now() + Duration::hours(24);
    queries::users::upsert_verification_token(
        &state.db,
        user_id,
        &token_hash,
        "email_verify",
        expires_at,
    )
    .await?;

    // Enqueue verification email via SQS (non-fatal — user can request resend)
    if let Err(e) = enqueue_email(
        &state,
        serde_json::json!({
            "kind": "email_verify",
            "user_id": user_id,
            "email": email,
            "username": username,
            "token": raw_token,
        }),
    )
    .await
    {
        tracing::warn!(error = %e, "failed to enqueue verification email; user can request resend");
    }

    Ok((
        StatusCode::CREATED,
        Json(json!({ "message": "registered; check your email to verify your account" })),
    ))
}

#[derive(Deserialize)]
struct ResendVerificationBody {
    email: Option<String>,
    username: Option<String>,
}

async fn resend_verification(
    State(state): State<AppState>,
    Json(body): Json<ResendVerificationBody>,
) -> Result<Json<Value>, DalError> {
    // Always return success to avoid account/email enumeration.
    let user = if let Some(email) = body.email.as_deref() {
        let email = email.trim().to_lowercase();
        queries::users::get_by_email(&state.db, &email).await?
    } else if let Some(username) = body.username.as_deref() {
        let username = username.trim().to_lowercase();
        queries::users::get_by_username(&state.db, &username).await?
    } else {
        None
    };

    if let Some(user) = user {
        // If already verified we still return success; no new token required.
        if !user.email_verified {
            let raw_token = format!("ev_{}", hex::encode(Uuid::new_v4().as_bytes()));
            let token_hash = sha256_hex(&raw_token);
            let expires_at = Utc::now() + Duration::hours(24);

            queries::users::upsert_verification_token(
                &state.db,
                user.id,
                &token_hash,
                "email_verify",
                expires_at,
            )
            .await?;

            if let Err(e) = enqueue_email(
                &state,
                serde_json::json!({
                    "kind": "email_verify",
                    "user_id": user.id,
                    "email": user.email,
                    "username": user.username,
                    "token": raw_token,
                }),
            )
            .await
            {
                tracing::warn!(error = %e, "failed to enqueue verification resend email");
            }
        }
    }

    Ok(Json(json!({
        "message": "if that email exists, a verification link has been sent"
    })))
}

// ── Login ─────────────────────────────────────────────────────────────────────

#[derive(Deserialize)]
struct LoginBody {
    username: String,
    password: String,
}

async fn login(
    State(state): State<AppState>,
    Json(body): Json<LoginBody>,
) -> Result<(HeaderMap, Json<Value>), DalError> {
    let username = body.username.trim().to_lowercase();

    // Gate on email verification before hitting Cognito
    let db_user = queries::users::get_by_username(&state.db, &username)
        .await?
        .ok_or(DalError::Unauthorized)?;
    if !db_user.email_verified {
        return Err(DalError::Validation(
            "please verify your email address before logging in".into(),
        ));
    }

    let (_access, id_token, refresh) = state.cognito.sign_in(&username, &body.password).await?;

    let mut headers = HeaderMap::new();
    headers.insert(
        header::SET_COOKIE,
        format!(
            "{REFRESH_COOKIE_NAME}={}",
            cookie_attrs("/auth/refresh", 0, &state.cfg.base_url)
        )
        .parse()
        .unwrap(),
    );
    // httpOnly SameSite=Strict cookies — tokens never exposed to JS
    headers.append(
        header::SET_COOKIE,
        format!(
            "{ACCESS_COOKIE_NAME}={id_token}{}",
            cookie_attrs("/", 3600, &state.cfg.base_url)
        )
        .parse()
        .unwrap(),
    );
    headers.append(
        header::SET_COOKIE,
        format!(
            "{REFRESH_COOKIE_NAME}={refresh}{}",
            cookie_attrs("/", 2_592_000, &state.cfg.base_url)
        )
        .parse()
        .unwrap(),
    );

    Ok((
        headers,
        Json(serde_json::to_value(&db_user).unwrap_or_default()),
    ))
}

// ── Logout ────────────────────────────────────────────────────────────────────

async fn logout(
    State(state): State<AppState>,
    AuthUser(user): AuthUser,
) -> Result<(HeaderMap, Json<Value>), DalError> {
    let _ = state.cognito.admin_sign_out(&user.username).await;

    let mut headers = HeaderMap::new();
    headers.insert(
        header::SET_COOKIE,
        format!(
            "{ACCESS_COOKIE_NAME}={}",
            cookie_attrs("/", 0, &state.cfg.base_url)
        )
        .parse()
        .unwrap(),
    );
    headers.append(
        header::SET_COOKIE,
        format!(
            "{REFRESH_COOKIE_NAME}={}",
            cookie_attrs("/auth/refresh", 0, &state.cfg.base_url)
        )
        .parse()
        .unwrap(),
    );
    headers.append(
        header::SET_COOKIE,
        format!(
            "{REFRESH_COOKIE_NAME}={}",
            cookie_attrs("/", 0, &state.cfg.base_url)
        )
        .parse()
        .unwrap(),
    );

    Ok((headers, Json(json!({ "message": "logged out" }))))
}

// ── Refresh ───────────────────────────────────────────────────────────────────

#[derive(Deserialize)]
struct RefreshBody {
    refresh_token: String,
}

async fn refresh(
    State(state): State<AppState>,
    headers: HeaderMap,
    body: Bytes,
) -> Result<(HeaderMap, Json<Value>), DalError> {
    let refresh_token = resolve_refresh_token(&headers, &body)?;
    let (_access, id) = state.cognito.refresh(&refresh_token).await?;

    let mut headers = HeaderMap::new();
    headers.insert(
        header::SET_COOKIE,
        format!(
            "{ACCESS_COOKIE_NAME}={id}{}",
            cookie_attrs("/", 3600, &state.cfg.base_url)
        )
        .parse()
        .unwrap(),
    );

    Ok((headers, Json(json!({ "message": "refreshed" }))))
}

// ── Verify email ──────────────────────────────────────────────────────────────

#[derive(Deserialize)]
struct VerifyParams {
    token: String,
}

async fn verify_email(
    State(state): State<AppState>,
    axum::extract::Query(params): axum::extract::Query<VerifyParams>,
) -> Result<Json<Value>, DalError> {
    let hash = sha256_hex(&params.token);
    let user_id = queries::users::consume_verification_token(&state.db, &hash, "email_verify")
        .await?
        .ok_or(DalError::Validation(
            "invalid or expired verification token".into(),
        ))?;

    queries::users::set_email_verified(&state.db, user_id).await?;

    // Confirm the user in Cognito so they can log in
    if let Some(user) = queries::users::get_by_id(&state.db, user_id).await? {
        let _ = state.cognito.admin_confirm_user(&user.username).await;
    }

    Ok(Json(
        json!({ "message": "email verified; you may now log in" }),
    ))
}

// ── Forgot password ───────────────────────────────────────────────────────────

#[derive(Deserialize)]
struct ForgotPasswordBody {
    email: String,
}

async fn forgot_password(
    State(state): State<AppState>,
    Json(body): Json<ForgotPasswordBody>,
) -> Result<Json<Value>, DalError> {
    // Always return 200 to prevent email enumeration
    let email = body.email.trim().to_lowercase();
    if let Some(user) = queries::users::get_by_email(&state.db, &email).await? {
        let raw_token = format!("pr_{}", hex::encode(Uuid::new_v4().as_bytes()));
        let token_hash = sha256_hex(&raw_token);
        let expires_at = Utc::now() + Duration::hours(1);

        let _ = queries::users::upsert_verification_token(
            &state.db,
            user.id,
            &token_hash,
            "password_reset",
            expires_at,
        )
        .await;

        let _ = enqueue_email(
            &state,
            serde_json::json!({
                "kind": "password_reset",
                "user_id": user.id,
                "email": email,
                "username": user.username,
                "token": raw_token,
            }),
        )
        .await;
    }

    Ok(Json(
        json!({ "message": "if that email exists, a password reset link has been sent" }),
    ))
}

// ── Reset password ────────────────────────────────────────────────────────────

#[derive(Deserialize)]
struct ResetPasswordBody {
    token: String,
    new_password: String,
}

async fn reset_password(
    State(state): State<AppState>,
    Json(body): Json<ResetPasswordBody>,
) -> Result<Json<Value>, DalError> {
    if body.new_password.len() < 8 {
        return Err(DalError::Validation(
            "password must be at least 8 chars".into(),
        ));
    }

    let hash = sha256_hex(&body.token);
    let user_id = queries::users::consume_verification_token(&state.db, &hash, "password_reset")
        .await?
        .ok_or(DalError::Validation(
            "invalid or expired reset token".into(),
        ))?;

    let user = queries::users::get_by_id(&state.db, user_id)
        .await?
        .ok_or(DalError::Unauthorized)?;

    state
        .cognito
        .admin_set_password(&user.username, &body.new_password)
        .await?;

    Ok(Json(
        json!({ "message": "password reset; you may now log in" }),
    ))
}

// ── Me ────────────────────────────────────────────────────────────────────────

async fn me(AuthUser(user): AuthUser) -> Json<Value> {
    Json(serde_json::to_value(&user).unwrap_or_default())
}

fn resolve_refresh_token(headers: &HeaderMap, body: &[u8]) -> Result<String, DalError> {
    if !body.is_empty() {
        let parsed: RefreshBody = serde_json::from_slice(body)
            .map_err(|e| DalError::Validation(format!("invalid refresh request body: {e}")))?;
        let refresh_token = parsed.refresh_token.trim();
        if refresh_token.is_empty() {
            return Err(DalError::Unauthorized);
        }
        return Ok(refresh_token.to_string());
    }

    extract_cookie(headers, REFRESH_COOKIE_NAME).ok_or(DalError::Unauthorized)
}

fn extract_cookie(headers: &HeaderMap, name: &str) -> Option<String> {
    let cookies = headers.get(header::COOKIE)?.to_str().ok()?;

    cookies.split(';').find_map(|cookie| {
        let (key, value) = cookie.trim().split_once('=')?;
        if key == name {
            Some(value.to_string())
        } else {
            None
        }
    })
}

// ── Helper ────────────────────────────────────────────────────────────────────

async fn enqueue_email(state: &AppState, payload: serde_json::Value) -> Result<(), DalError> {
    let kind = payload["kind"].as_str().unwrap_or("unknown");
    let body = serde_json::to_string(&payload).unwrap();
    let mut last_err: Option<String> = None;

    // Retry transient transport problems (e.g., temporary dispatch failures).
    for attempt in 1..=3 {
        let dedup_id = format!("{kind}:{}", Uuid::new_v4());
        match state
            .sqs_client
            .send_message()
            .queue_url(&state.sqs_url)
            .message_body(&body)
            .message_group_id("email")
            .message_deduplication_id(dedup_id)
            .send()
            .await
        {
            Ok(_) => return Ok(()),
            Err(e) => {
                last_err = Some(e.to_string());
                if attempt < 3 {
                    tracing::warn!(
                        attempt,
                        kind,
                        queue_url = %state.sqs_url,
                        cause = %e,
                        "sqs enqueue failed; retrying"
                    );
                    tokio::time::sleep(std::time::Duration::from_millis(250 * attempt as u64))
                        .await;
                }
            }
        }
    }

    Err(DalError::Sqs(format!(
        "failed to enqueue kind={kind} queue_url={} region={} cause={}",
        state.sqs_url,
        state.cfg.aws_region,
        last_err.unwrap_or_else(|| "unknown error".to_string())
    )))
}
