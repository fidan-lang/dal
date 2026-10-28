use axum::{
    Json, Router,
    body::Bytes,
    extract::State,
    http::{HeaderMap, StatusCode, header},
    routing::{get, post},
};
use chrono::{Duration, Timelike, Utc};
use serde::Deserialize;
use serde_json::{Value, json};
use url::Url;
use uuid::Uuid;

use dal_auth::api_token::hash_token as sha256_hex;
use dal_common::error::DalError;
use dal_db::queries;

use crate::{
    extractors::{AuthUser, VerifiedSession},
    responses::CurrentUserResponse,
    state::AppState,
};

pub fn router() -> Router<AppState> {
    Router::new()
        .merge(super::mfa::router())
        .route("/auth/register", post(register))
        .route("/auth/resend-verification", post(resend_verification))
        .route("/auth/login", post(login))
        .route("/auth/logout", post(logout))
        .route("/auth/refresh", post(refresh))
        .route("/auth/verify-email", get(verify_email))
        .route("/auth/forgot-password", post(forgot_password))
        .route("/auth/reset-password", post(reset_password))
        .route("/auth/me", get(me))
        .layer(axum::middleware::map_response(super::mfa::no_store))
}

const ACCESS_COOKIE_NAME: &str = "dal_access_token";
const REFRESH_COOKIE_NAME: &str = "dal_refresh_token";

pub(super) fn cookie_attrs(path: &str, max_age: i64, base_url: &str) -> String {
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

    let user_id = Uuid::new_v4();
    let raw_token = format!("ev_{}", hex::encode(Uuid::new_v4().as_bytes()));
    let token_hash = sha256_hex(&raw_token);
    let expires_at = Utc::now() + Duration::hours(24);
    // Both DB records must exist before registration can be committed.
    let pending = async {
        let mut transaction = state.db.begin().await?;
        queries::users::create(
            &mut *transaction,
            user_id,
            &username,
            &email,
            &cognito_sub,
            body.display_name.as_deref(),
        )
        .await?;
        queries::users::upsert_verification_token(
            &mut *transaction,
            user_id,
            &token_hash,
            "email_verify",
            expires_at,
        )
        .await?;
        Ok::<_, DalError>(transaction)
    }
    .await;
    let transaction = match pending {
        Ok(transaction) => transaction,
        Err(error) => {
            if state
                .cognito
                .admin_delete_user_if_exists(&username)
                .await
                .is_err()
            {
                tracing::error!(
                    username,
                    operation = "registration:rollback",
                    "new Cognito identity requires reconciliation after DB failure"
                );
            }
            return Err(error);
        }
    };
    // On an ambiguous commit retain the provider identity; deleting it could break
    // a registration the database actually committed.
    if let Err(error) = transaction.commit().await {
        tracing::error!(
            username,
            operation = "registration:commit",
            "registration outcome requires reconciliation"
        );
        return Err(error.into());
    }

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

    // Revocation rejects the entire cutoff second because Cognito iat is integral.
    // Authenticate only after that second has ended; otherwise a successful fresh
    // login immediately returns a token that our own authorization must reject.
    if let Some(cutoff) = db_user.session_invalid_before {
        let next_epoch = cutoff.with_nanosecond(0).unwrap() + Duration::seconds(1);
        if let Ok(remaining) = (next_epoch - Utc::now()).to_std() {
            tokio::time::sleep(remaining).await;
        }
    }

    match state
        .cognito
        .begin_sign_in(&username, &body.password)
        .await?
    {
        dal_auth::cognito::Authentication::Complete(tokens) => {
            finish_login(&state, tokens, db_user.id, false).await
        }
        dal_auth::cognito::Authentication::Totp { username, session } => {
            let handle = state.mfa.insert(
                crate::mfa::Pending::new(
                    db_user.id,
                    crate::mfa::PendingKind::Login { username, session },
                )
                .at_epoch(db_user.session_invalid_before),
            )?;
            Ok((
                super::mfa::pending_headers(
                    crate::mfa::PENDING_COOKIE,
                    &handle,
                    &state.cfg.base_url,
                )?,
                Json(
                    json!({ "mfa_required": true, "expires_in": crate::mfa::PENDING_TTL.as_secs() }),
                ),
            ))
        }
    }
}

pub(super) async fn finish_login(
    state: &AppState,
    tokens: dal_auth::cognito::AuthTokens,
    expected_user: Uuid,
    mfa_completed: bool,
) -> Result<(HeaderMap, Json<Value>), DalError> {
    let (db_user, _) = crate::extractors::validated_identity(state, &tokens.id).await?;
    if db_user.id != expected_user || !db_user.email_verified {
        return Err(DalError::Unauthorized);
    }
    if !mfa_completed
        && (queries::users::mfa_required(&state.db, db_user.id).await?
            || state.cognito.totp_enabled(&db_user.username).await?)
    {
        return Err(DalError::Unauthorized);
    }
    if mfa_completed {
        crate::extractors::bind_mfa_session(state, &tokens.id, &tokens.refresh, false).await?;
    }
    let id_token = tokens.id;
    let refresh = tokens.refresh;
    let mut headers =
        super::mfa::pending_headers(crate::mfa::PENDING_COOKIE, "", &state.cfg.base_url)?;
    headers.append(
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
        Json(serde_json::to_value(CurrentUserResponse::from(db_user)).unwrap_or_default()),
    ))
}

// ── Logout ────────────────────────────────────────────────────────────────────

async fn logout(
    State(state): State<AppState>,
    VerifiedSession(user): VerifiedSession,
) -> Result<(HeaderMap, Json<Value>), DalError> {
    state.mfa.revoke(user.id);
    revoke_global_session(&state.cognito, &user.username).await?;
    // Include tokens issued while the provider request was in flight. The
    // cutoff is monotonic; whole-second iat equality deliberately fails closed.
    let mut tx = dal_db::actor_transaction(&state.db, user.id, None).await?;
    queries::users::advance_session_invalid_before(&mut *tx, user.id, Utc::now()).await?;
    tx.commit().await?;

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

/// A successful logout means Cognito has invalidated the user's refresh tokens.
/// Do not clear browser cookies and report success when that provider operation fails.
async fn revoke_global_session(
    cognito: &dal_auth::CognitoClient,
    username: &str,
) -> Result<(), DalError> {
    cognito.admin_sign_out(username).await
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
    let (user, _) = crate::extractors::validated_identity(&state, &id).await?;
    if queries::users::mfa_required(&state.db, user.id).await? {
        crate::extractors::bind_mfa_session(&state, &id, &refresh_token, true).await?;
    }

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
    let mut transaction = state.db.begin().await?;
    let hash = sha256_hex(&params.token);
    let user_id =
        queries::users::consume_verification_token(&mut *transaction, &hash, "email_verify")
            .await?
            .ok_or(DalError::Validation(
                "invalid or expired verification token".into(),
            ))?;

    // The hash-consuming database capability marks email verified atomically.

    // Confirm the user in Cognito so they can log in
    if let Some(user) = queries::users::auth_by_id(&mut *transaction, user_id).await? {
        state.cognito.admin_confirm_user(&user.username).await?;
    } else {
        return Err(DalError::Validation("account no longer exists".into()));
    }

    transaction.commit().await?;
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

        match queries::users::upsert_verification_token(
            &state.db,
            user.id,
            &token_hash,
            "password_reset",
            expires_at,
        )
        .await
        {
            Ok(()) => {
                if enqueue_email(
                    &state,
                    serde_json::json!({
                        "kind": "password_reset", "user_id": user.id, "email": email,
                        "username": user.username, "token": raw_token,
                    }),
                )
                .await
                .is_err()
                {
                    tracing::error!(
                        operation = "password_reset:enqueue",
                        "reset delivery failed; caller may request a new link"
                    );
                }
            }
            Err(_) => {
                // Preserve the generic public response without sending an invalid link.
                tracing::error!(
                    operation = "password_reset:store",
                    "reset persistence failed; no email queued"
                );
            }
        }
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

    let mut transaction = state.db.begin().await?;
    let hash = sha256_hex(&body.token);
    let user_id =
        queries::users::consume_verification_token(&mut *transaction, &hash, "password_reset")
            .await?
            .ok_or(DalError::Validation(
                "invalid or expired reset token".into(),
            ))?;

    let user = queries::users::auth_by_id(&mut *transaction, user_id)
        .await?
        .ok_or(DalError::Unauthorized)?;

    state
        .cognito
        .admin_set_password(&user.username, &body.new_password)
        .await?;
    dal_db::bind_actor(&mut transaction, user.id, None).await?;
    queries::users::advance_session_invalid_before(&mut *transaction, user.id, Utc::now()).await?;

    transaction.commit().await?;
    Ok(Json(
        json!({ "message": "password reset; you may now log in" }),
    ))
}

// ── Me ────────────────────────────────────────────────────────────────────────

async fn me(AuthUser(user): AuthUser) -> Json<Value> {
    Json(serde_json::to_value(CurrentUserResponse::from(user)).unwrap_or_default())
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

#[cfg(test)]
mod logout_tests {
    use super::*;
    use aws_config::{BehaviorVersion, Region};
    use aws_sdk_sqs::config::Credentials;
    use axum::{
        Json, Router,
        http::{HeaderMap, StatusCode},
    };
    use serde_json::json;
    use std::sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    };

    #[tokio::test]
    async fn logout_propagates_a_real_cognito_global_sign_out_failure() {
        let requests = Arc::new(AtomicUsize::new(0));
        let observed = requests.clone();
        let provider = Router::new().fallback(move |headers: HeaderMap| {
            let observed = observed.clone();
            async move {
                assert_eq!(
                    headers
                        .get("x-amz-target")
                        .and_then(|value| value.to_str().ok())
                        .and_then(|target| target.rsplit('.').next()),
                    Some("AdminUserGlobalSignOut")
                );
                observed.fetch_add(1, Ordering::SeqCst);
                (
                    StatusCode::BAD_GATEWAY,
                    Json(json!({
                        "__type": "ServiceUnavailableException",
                        "message": "fixture provider outage"
                    })),
                )
            }
        });
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let endpoint = format!("http://{}", listener.local_addr().unwrap());
        let provider_task = tokio::spawn(async move {
            axum::serve(listener, provider).await.unwrap();
        });

        let aws = aws_config::defaults(BehaviorVersion::latest())
            .region(Region::new("eu-central-1"))
            .credentials_provider(Credentials::new("test", "test", None, None, "local-test"))
            .retry_config(aws_config::retry::RetryConfig::disabled())
            .load()
            .await;
        let cognito = dal_auth::CognitoClient::from_sdk_config(
            &aws,
            "local_logout_pool".into(),
            "local_logout_client".into(),
            Some(&endpoint),
        );

        let error = revoke_global_session(&cognito, "logout-fixture")
            .await
            .expect_err("a failed global sign-out must not be reported as logout success");
        assert!(matches!(error, DalError::Cognito(_)));
        assert_eq!(requests.load(Ordering::SeqCst), 1);
        provider_task.abort();
    }
}
