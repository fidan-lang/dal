use crate::{
    extractors::{VerifiedSession, validated_identity},
    mfa::{PENDING_COOKIE, PENDING_TTL, Pending, PendingKind, SETUP_COOKIE},
    state::AppState,
};
use axum::{
    Json, Router,
    extract::State,
    http::{HeaderMap, header},
    routing::{get, post},
};
use dal_auth::cognito::{AuthTokens, Authentication};
use dal_common::error::DalError;
use dal_db::{models::User, queries};
use serde::Deserialize;
use serde_json::{Value, json};

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/auth/mfa/status", get(status))
        .route("/auth/mfa/challenge", post(challenge))
        .route("/auth/mfa/setup", post(setup))
        .route("/auth/mfa/verify", post(verify))
        .route("/auth/mfa/disable", post(disable))
        .route("/auth/mfa/recovery-codes", post(generate_recovery_codes))
        .route("/auth/mfa/recover", post(recover_with_code))
}
pub async fn no_store(mut response: axum::response::Response) -> axum::response::Response {
    response
        .headers_mut()
        .insert(header::CACHE_CONTROL, "no-store".parse().unwrap());
    response
        .headers_mut()
        .insert(header::PRAGMA, "no-cache".parse().unwrap());
    response
        .headers_mut()
        .insert(header::REFERRER_POLICY, "no-referrer".parse().unwrap());
    response
}
pub(super) fn pending_headers(name: &str, value: &str, base: &str) -> Result<HeaderMap, DalError> {
    let age = if value.is_empty() {
        0
    } else {
        PENDING_TTL.as_secs() as i64
    };
    let mut headers = HeaderMap::new();
    headers.insert(
        header::SET_COOKIE,
        format!(
            "{name}={value}{}",
            super::auth::cookie_attrs("/auth", age, base)
        )
        .parse()
        .map_err(|_| DalError::Unauthorized)?,
    );
    Ok(headers)
}
fn cookie(headers: &HeaderMap, name: &str) -> Result<String, DalError> {
    headers
        .get_all(header::COOKIE)
        .iter()
        .filter_map(|v| v.to_str().ok())
        .flat_map(|v| v.split(';'))
        .find_map(|v| {
            v.trim()
                .split_once('=')
                .filter(|(key, _)| *key == name)
                .map(|(_, v)| v.to_owned())
        })
        .filter(|v| v.len() == 68)
        .ok_or(DalError::Unauthorized)
}
#[derive(Deserialize)]
struct Code {
    code: String,
}
async fn challenge(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(body): Json<Code>,
) -> Result<(HeaderMap, Json<Value>), DalError> {
    let handle = cookie(&headers, PENDING_COOKIE)?;
    let pending = state.mfa.take(&handle)?;
    let PendingKind::Login { username, session } = &pending.kind else {
        return Err(DalError::Unauthorized);
    };
    let tokens = match state
        .cognito
        .complete_totp(username, session, &body.code)
        .await
    {
        Ok(tokens) => tokens,
        Err(error) => {
            state.mfa.retry(&handle, pending);
            return Err(error);
        }
    };
    let user = queries::users::auth_by_id(&state.db, pending.user_id)
        .await?
        .ok_or(DalError::Unauthorized)?;
    if !pending.valid_for(&user) {
        return Err(DalError::Unauthorized);
    }
    super::auth::finish_login(&state, tokens, pending.user_id, true).await
}
async fn status(
    State(state): State<AppState>,
    VerifiedSession(user): VerifiedSession,
) -> Result<Json<Value>, DalError> {
    let enabled = queries::users::mfa_required(&state.db, user.id).await?
        || state.cognito.totp_enabled(&user.username).await?;
    Ok(Json(
        json!({"enabled":enabled,"available":state.cfg.totp_enabled,"expires_in":PENDING_TTL.as_secs()}),
    ))
}
#[derive(Deserialize)]
struct Reauthenticate {
    password: String,
    current_code: Option<String>,
}
async fn reauthenticate(
    state: &AppState,
    user: &User,
    body: &Reauthenticate,
    required: bool,
) -> Result<AuthTokens, DalError> {
    let tokens = match state
        .cognito
        .begin_sign_in(&user.username, &body.password)
        .await?
    {
        Authentication::Complete(tokens) if !required => tokens,
        Authentication::Totp { username, session } => {
            state
                .cognito
                .complete_totp(
                    &username,
                    &session,
                    body.current_code.as_deref().ok_or(DalError::Unauthorized)?,
                )
                .await?
        }
        _ => return Err(DalError::Unauthorized),
    };
    if validated_identity(state, &tokens.id).await?.0.id != user.id {
        return Err(DalError::Unauthorized);
    }
    Ok(tokens)
}
async fn setup(
    State(state): State<AppState>,
    VerifiedSession(user): VerifiedSession,
    Json(body): Json<Reauthenticate>,
) -> Result<(HeaderMap, Json<Value>), DalError> {
    if !state.cfg.totp_enabled {
        return Err(DalError::Validation(
            "Authenticator enrollment is not yet enabled by the registry operator".into(),
        ));
    }
    let _change = state.mfa.change()?;
    let mut tx = dal_db::actor_transaction(&state.db, user.id, None).await?;
    let required = queries::users::lock_mfa(&mut tx, user.id).await?
        || state.cognito.totp_enabled(&user.username).await?;
    let tokens = reauthenticate(&state, &user, &body, required).await?;
    // Serialize setup and preference changes across instances. Cognito retains the secret.
    state.mfa.revoke(user.id);
    let secret = state.cognito.associate_totp(&tokens.access).await?;
    let handle = state.mfa.insert(
        Pending::new(
            user.id,
            PendingKind::Setup {
                access: tokens.access,
            },
        )
        .at_epoch(user.session_invalid_before),
    )?;
    tx.commit().await?;
    let mut uri = url::Url::parse("otpauth://totp/").map_err(|_| DalError::Unauthorized)?;
    uri.set_path(&format!("DAL:{}", user.username));
    uri.query_pairs_mut()
        .append_pair("secret", &secret)
        .append_pair("issuer", "DAL")
        .append_pair("algorithm", "SHA1")
        .append_pair("digits", "6")
        .append_pair("period", "30");
    Ok((
        pending_headers(SETUP_COOKIE, &handle, &state.cfg.base_url)?,
        Json(json!({"secret":secret,"uri":uri.as_str(),"expires_in":PENDING_TTL.as_secs()})),
    ))
}
async fn verify(
    State(state): State<AppState>,
    VerifiedSession(user): VerifiedSession,
    headers: HeaderMap,
    Json(body): Json<Code>,
) -> Result<(HeaderMap, Json<Value>), DalError> {
    let handle = cookie(&headers, SETUP_COOKIE)?;
    let pending = state.mfa.take(&handle)?;
    if !pending.valid_for(&user) {
        return Err(DalError::Unauthorized);
    }
    let PendingKind::Setup { access } = &pending.kind else {
        return Err(DalError::Unauthorized);
    };
    let _change = state.mfa.change()?;
    let mut tx = dal_db::actor_transaction(&state.db, user.id, None).await?;
    let was_required = queries::users::lock_mfa(&mut tx, user.id).await?;
    // Commit the downgrade barrier before Cognito can mutate the factor. If the
    // provider outcome is ambiguous, never silently allow password-only tokens.
    queries::users::set_mfa_required(&mut tx, user.id, true).await?;
    tx.commit().await?;
    if !state.cognito.verify_totp(access, &body.code).await? {
        if !was_required && !state.cognito.totp_enabled(&user.username).await? {
            let mut rollback = dal_db::actor_transaction(&state.db, user.id, None).await?;
            queries::users::lock_mfa(&mut rollback, user.id).await?;
            let current = queries::users::lock_session(&mut rollback, user.id).await?;
            // A delayed provider read must not undo a completed concurrent change.
            if pending.valid_for(&current) {
                queries::users::set_mfa_required(&mut rollback, user.id, false).await?;
            }
            rollback.commit().await?;
        }
        state.mfa.retry(&handle, pending);
        return Err(DalError::Unauthorized);
    }
    state.cognito.set_totp(access, true).await?;
    let mut tx = dal_db::actor_transaction(&state.db, user.id, None).await?;
    queries::users::lock_mfa(&mut tx, user.id).await?;
    // An earlier failed verification may have rolled back its provisional guard.
    queries::users::set_mfa_required(&mut tx, user.id, true).await?;
    queries::users::advance_session_invalid_before(&mut *tx, user.id, chrono::Utc::now()).await?;
    sqlx::query("SELECT dal_security.clear_recovery_codes($1)")
        .bind(user.id)
        .execute(&mut *tx)
        .await?;
    tx.commit().await?;
    state.mfa.revoke(user.id);
    state.cognito.admin_sign_out(&user.username).await?;
    Ok((
        pending_headers(SETUP_COOKIE, "", &state.cfg.base_url)?,
        Json(
            json!({"message":"Authenticator enabled. Sign in again with your password and authenticator code."}),
        ),
    ))
}
async fn disable(
    State(state): State<AppState>,
    VerifiedSession(user): VerifiedSession,
    Json(body): Json<Reauthenticate>,
) -> Result<Json<Value>, DalError> {
    let _change = state.mfa.change()?;
    let mut tx = dal_db::actor_transaction(&state.db, user.id, None).await?;
    let required = queries::users::lock_mfa(&mut tx, user.id).await?
        || state.cognito.totp_enabled(&user.username).await?;
    if !required {
        return Err(DalError::Validation("Authenticator is not enabled".into()));
    }
    let tokens = reauthenticate(&state, &user, &body, true).await?;
    state.cognito.set_totp(&tokens.access, false).await?;
    queries::users::set_mfa_required(&mut tx, user.id, false).await?;
    sqlx::query("SELECT dal_security.clear_recovery_codes($1)")
        .bind(user.id)
        .execute(&mut *tx)
        .await?;
    queries::users::advance_session_invalid_before(&mut *tx, user.id, chrono::Utc::now()).await?;
    tx.commit().await?;
    state.mfa.revoke(user.id);
    state.cognito.admin_sign_out(&user.username).await?;
    Ok(Json(
        json!({"message":"Authenticator disabled. Sign in again."}),
    ))
}

#[derive(Deserialize)]
struct RecoveryCodeBody {
    code: String,
}

async fn generate_recovery_codes(
    State(state): State<AppState>,
    VerifiedSession(user): VerifiedSession,
    Json(body): Json<Reauthenticate>,
) -> Result<Json<Value>, DalError> {
    if !state.cfg.totp_enabled {
        return Err(DalError::Unauthorized);
    }
    let _change = state.mfa.change()?;
    // A leaked browser session alone cannot rotate recovery codes.
    if !queries::users::mfa_required(&state.db, user.id).await?
        || !state.cognito.totp_enabled(&user.username).await?
    {
        return Err(DalError::Unauthorized);
    }
    let _verified = reauthenticate(&state, &user, &body, true).await?;
    let generated: Vec<_> = (0..10).map(|_| dal_auth::generate_api_token()).collect();
    let hashes: Vec<String> = generated.iter().map(|token| token.hash.clone()).collect();
    let mut tx = dal_db::actor_transaction(&state.db, user.id, None).await?;
    sqlx::query("SELECT dal_security.replace_recovery_codes($1,$2)")
        .bind(user.id)
        .bind(&hashes)
        .execute(&mut *tx)
        .await?;
    tx.commit().await?;
    Ok(Json(json!({
        "codes": generated.into_iter().map(|token| token.raw).collect::<Vec<_>>(),
        "message": "Store these codes offline now. They will never be shown again."
    })))
}

async fn recover_with_code(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(body): Json<RecoveryCodeBody>,
) -> Result<(HeaderMap, Json<Value>), DalError> {
    let _change = state.mfa.change()?;
    let handle = cookie(&headers, PENDING_COOKIE)?;
    let pending = state.mfa.take(&handle)?;
    let PendingKind::Login { username, .. } = &pending.kind else {
        return Err(DalError::Unauthorized);
    };
    let user = queries::users::auth_by_id(&state.db, pending.user_id)
        .await?
        .ok_or(DalError::Unauthorized)?;
    if !pending.valid_for(&user) || !user.email_verified || username != &user.username {
        return Err(DalError::Unauthorized);
    }
    if !dal_auth::api_token::validate_api_token_format(&body.code) {
        state.mfa.retry(&handle, pending);
        return Err(DalError::Unauthorized);
    }
    let proof = dal_auth::generate_api_token();
    let consumed: bool = sqlx::query_scalar("SELECT dal_security.consume_recovery_code($1,$2,$3)")
        .bind(user.id)
        .bind(dal_auth::hash_token(&body.code))
        .bind(&proof.hash)
        .fetch_one(&state.db)
        .await?;
    if !consumed {
        state.mfa.retry(&handle, pending);
        return Err(DalError::Unauthorized);
    }
    // Fail closed: code is consumed before provider I/O. Cognito must confirm
    // factor removal before the durable MFA downgrade guard can be cleared.
    state.cognito.admin_delete_totp(&user.username).await?;
    sqlx::query("SELECT dal_security.finish_recovery($1,$2)")
        .bind(user.id)
        .bind(&proof.hash)
        .execute(&state.db)
        .await?;
    state.mfa.revoke(user.id);
    state.cognito.admin_sign_out(&user.username).await?;
    Ok((
        pending_headers(PENDING_COOKIE, "", &state.cfg.base_url)?,
        Json(
            json!({"message":"Recovery accepted. Sign in again and set up a new authenticator. All prior sessions and API tokens were revoked."}),
        ),
    ))
}
