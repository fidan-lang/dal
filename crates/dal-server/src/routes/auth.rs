use axum::{
    extract::State,
    http::{header, HeaderMap, StatusCode},
    routing::{get, post},
    Json, Router,
};
use chrono::{Duration, Utc};
use serde::Deserialize;
use serde_json::{json, Value};
use uuid::Uuid;

use dal_auth::api_token::hash_token as sha256_hex;
use dal_common::error::DalError;
use dal_db::queries;

use crate::{extractors::AuthUser, state::AppState};

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/auth/register", post(register))
        .route("/auth/login", post(login))
        .route("/auth/logout", post(logout))
        .route("/auth/refresh", post(refresh))
        .route("/auth/verify-email", get(verify_email))
        .route("/auth/forgot-password", post(forgot_password))
        .route("/auth/reset-password", post(reset_password))
        .route("/auth/me", get(me))
}

// ── Register ─────────────────────────────────────────────────────────────────

#[derive(Deserialize)]
struct RegisterBody {
    username:     String,
    email:        String,
    password:     String,
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
    if !username.chars().all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_') {
        return Err(DalError::Validation(
            "username may only contain letters, digits, hyphens and underscores".into(),
        ));
    }
    if body.password.len() < 8 {
        return Err(DalError::Validation("password must be at least 8 chars".into()));
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
    queries::users::create(
        &state.db,
        user_id,
        &username,
        &email,
        &cognito_sub,
        body.display_name.as_deref(),
    )
    .await?;

    // Generate + store email verification token
    let raw_token = format!("ev_{}", hex::encode(Uuid::new_v4().as_bytes()));
    let token_hash = sha256_hex(&raw_token);
    let expires_at = Utc::now() + Duration::hours(24);
    queries::users::upsert_verification_token(
        &state.db, user_id, &token_hash, "email_verify", expires_at,
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

    Ok((StatusCode::CREATED, Json(json!({ "message": "registered; check your email to verify your account" }))))
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

    let (access, _id_token, refresh) = state
        .cognito
        .sign_in(&username, &body.password)
        .await?;

    let mut headers = HeaderMap::new();
    // httpOnly SameSite=Strict cookies — tokens never exposed to JS
    headers.insert(
        header::SET_COOKIE,
        format!(
            "dal_access_token={access}; HttpOnly; Secure; SameSite=Strict; Path=/; Max-Age=3600"
        ).parse().unwrap(),
    );
    headers.append(
        header::SET_COOKIE,
        format!(
            "dal_refresh_token={refresh}; HttpOnly; Secure; SameSite=Strict; Path=/auth/refresh; Max-Age=2592000"
        ).parse().unwrap(),
    );

    Ok((headers, Json(json!({ "message": "logged in" }))))
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
        "dal_access_token=; HttpOnly; Secure; SameSite=Strict; Path=/; Max-Age=0".parse().unwrap(),
    );
    headers.append(
        header::SET_COOKIE,
        "dal_refresh_token=; HttpOnly; Secure; SameSite=Strict; Path=/auth/refresh; Max-Age=0".parse().unwrap(),
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
    Json(body): Json<RefreshBody>,
) -> Result<(HeaderMap, Json<Value>), DalError> {
    let (access, _id) = state.cognito.refresh(&body.refresh_token).await?;

    let mut headers = HeaderMap::new();
    headers.insert(
        header::SET_COOKIE,
        format!(
            "dal_access_token={access}; HttpOnly; Secure; SameSite=Strict; Path=/; Max-Age=3600"
        ).parse().unwrap(),
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
        .ok_or(DalError::Validation("invalid or expired verification token".into()))?;

    queries::users::set_email_verified(&state.db, user_id).await?;

    // Confirm the user in Cognito so they can log in
    if let Some(user) = queries::users::get_by_id(&state.db, user_id).await? {
        let _ = state.cognito.admin_confirm_user(&user.username).await;
    }

    Ok(Json(json!({ "message": "email verified; you may now log in" })))
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
            &state.db, user.id, &token_hash, "password_reset", expires_at,
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

    Ok(Json(json!({ "message": "if that email exists, a password reset link has been sent" })))
}

// ── Reset password ────────────────────────────────────────────────────────────

#[derive(Deserialize)]
struct ResetPasswordBody {
    token:        String,
    new_password: String,
}

async fn reset_password(
    State(state): State<AppState>,
    Json(body): Json<ResetPasswordBody>,
) -> Result<Json<Value>, DalError> {
    if body.new_password.len() < 8 {
        return Err(DalError::Validation("password must be at least 8 chars".into()));
    }

    let hash = sha256_hex(&body.token);
    let user_id = queries::users::consume_verification_token(&state.db, &hash, "password_reset")
        .await?
        .ok_or(DalError::Validation("invalid or expired reset token".into()))?;

    let user = queries::users::get_by_id(&state.db, user_id)
        .await?
        .ok_or(DalError::Unauthorized)?;

    state.cognito
        .admin_set_password(&user.username, &body.new_password)
        .await?;

    Ok(Json(json!({ "message": "password reset; you may now log in" })))
}

// ── Me ────────────────────────────────────────────────────────────────────────

async fn me(AuthUser(user): AuthUser) -> Json<Value> {
    Json(serde_json::to_value(&user).unwrap_or_default())
}

// ── Helper ────────────────────────────────────────────────────────────────────

async fn enqueue_email(state: &AppState, payload: serde_json::Value) -> Result<(), DalError> {
    let body = serde_json::to_string(&payload).unwrap();
    state
        .sqs_client
        .send_message()
        .queue_url(&state.sqs_url)
        .message_body(&body)
        .message_group_id("email")
        .send()
        .await
        .map_err(|e| DalError::Sqs(e.to_string()))?;
    Ok(())
}
