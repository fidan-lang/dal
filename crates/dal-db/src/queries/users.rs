use crate::models::{User, UserPublic};
use dal_common::Result as DalResult;
use sqlx::PgPool;
use uuid::Uuid;

pub async fn create(
    pool: impl sqlx::Executor<'_, Database = sqlx::Postgres>,
    id: Uuid,
    username: &str,
    email: &str,
    cognito_sub: &str,
    display_name: Option<&str>,
) -> DalResult<User> {
    let user = sqlx::query_as::<_, User>(
        "INSERT INTO users (id, username, email, cognito_sub, display_name)
         VALUES ($1, $2, $3, $4, $5)
         RETURNING *",
    )
    .bind(id)
    .bind(username)
    .bind(email)
    .bind(cognito_sub)
    .bind(display_name)
    .fetch_one(pool)
    .await?;
    Ok(user)
}

pub async fn get_by_id(
    pool: impl sqlx::Executor<'_, Database = sqlx::Postgres>,
    id: Uuid,
) -> DalResult<Option<User>> {
    let user = sqlx::query_as::<_, User>("SELECT * FROM users WHERE id = $1")
        .bind(id)
        .fetch_optional(pool)
        .await?;
    Ok(user)
}

pub async fn get_by_username(pool: &PgPool, username: &str) -> DalResult<Option<User>> {
    let user = sqlx::query_as::<_, User>("SELECT * FROM users WHERE lower(username) = lower($1)")
        .bind(username)
        .fetch_optional(pool)
        .await?;
    Ok(user)
}

pub async fn get_by_email(pool: &PgPool, email: &str) -> DalResult<Option<User>> {
    let user = sqlx::query_as::<_, User>("SELECT * FROM users WHERE lower(email) = lower($1)")
        .bind(email)
        .fetch_optional(pool)
        .await?;
    Ok(user)
}

pub async fn list_stale_unverified(
    pool: &PgPool,
    older_than: chrono::DateTime<chrono::Utc>,
    limit: i64,
) -> DalResult<Vec<User>> {
    let users = sqlx::query_as::<_, User>(
        "SELECT *
         FROM users
         WHERE email_verified = false
           AND created_at < $1
         ORDER BY created_at ASC
         LIMIT $2",
    )
    .bind(older_than)
    .bind(limit)
    .fetch_all(pool)
    .await?;
    Ok(users)
}

/// Recheck eligibility while holding the same row lock as verification updates.
pub async fn lock_stale_unverified(
    connection: impl sqlx::Executor<'_, Database = sqlx::Postgres>,
    user_id: Uuid,
    older_than: chrono::DateTime<chrono::Utc>,
) -> DalResult<Option<User>> {
    Ok(sqlx::query_as::<_, User>(
        "SELECT * FROM users WHERE id = $1 AND email_verified = false AND created_at < $2 FOR UPDATE",
    ).bind(user_id).bind(older_than).fetch_optional(connection).await?)
}

pub async fn get_by_cognito_sub(pool: &PgPool, sub: &str) -> DalResult<Option<User>> {
    let user = sqlx::query_as::<_, User>("SELECT * FROM users WHERE cognito_sub = $1")
        .bind(sub)
        .fetch_optional(pool)
        .await?;
    Ok(user)
}

pub async fn get_public_by_username(
    pool: &PgPool,
    username: &str,
) -> DalResult<Option<UserPublic>> {
    let user = sqlx::query_as::<_, UserPublic>(
        "SELECT id, username, display_name, avatar_url, bio, website, created_at
         FROM users WHERE lower(username) = lower($1)",
    )
    .bind(username)
    .fetch_optional(pool)
    .await?;
    Ok(user)
}

pub async fn set_email_verified(
    pool: impl sqlx::Executor<'_, Database = sqlx::Postgres>,
    user_id: Uuid,
) -> DalResult<()> {
    sqlx::query("UPDATE users SET email_verified = true, updated_at = now() WHERE id = $1")
        .bind(user_id)
        .execute(pool)
        .await?;
    Ok(())
}

/// Advance the JWT revocation cutoff without allowing an older request to move
/// it backwards.
pub async fn advance_session_invalid_before(
    pool: impl sqlx::Executor<'_, Database = sqlx::Postgres>,
    user_id: Uuid,
    invalid_before: chrono::DateTime<chrono::Utc>,
) -> DalResult<()> {
    sqlx::query(
        "WITH revoked AS (UPDATE users
         SET session_invalid_before = CASE
                 WHEN session_invalid_before IS NULL OR session_invalid_before < $2 THEN $2
                 ELSE session_invalid_before
             END,
             updated_at = now()
         WHERE id = $1 RETURNING id)
         DELETE FROM mfa_sessions WHERE user_id IN (SELECT id FROM revoked)",
    )
    .bind(user_id)
    .bind(invalid_before)
    .execute(pool)
    .await?;
    Ok(())
}

pub async fn delete_unverified(
    pool: impl sqlx::Executor<'_, Database = sqlx::Postgres>,
    user_id: Uuid,
) -> DalResult<bool> {
    let result = sqlx::query("DELETE FROM users WHERE id = $1 AND email_verified = false")
        .bind(user_id)
        .execute(pool)
        .await?;
    Ok(result.rows_affected() > 0)
}

pub async fn update_profile(
    pool: &PgPool,
    user_id: Uuid,
    display_name: Option<&str>,
    bio: Option<&str>,
    website: Option<&str>,
    avatar_url: Option<&str>,
) -> DalResult<User> {
    let user = sqlx::query_as::<_, User>(
        "UPDATE users
         SET display_name = CASE WHEN $2 IS NULL THEN display_name ELSE NULLIF($2, '') END,
             bio          = CASE WHEN $3 IS NULL THEN bio ELSE NULLIF($3, '') END,
             website      = CASE WHEN $4 IS NULL THEN website ELSE NULLIF($4, '') END,
             avatar_url   = CASE WHEN $5 IS NULL THEN avatar_url ELSE NULLIF($5, '') END,
             updated_at   = now()
         WHERE id = $1
         RETURNING *",
    )
    .bind(user_id)
    .bind(display_name)
    .bind(bio)
    .bind(website)
    .bind(avatar_url)
    .fetch_one(pool)
    .await?;
    Ok(user)
}

pub async fn username_exists(pool: &PgPool, username: &str) -> DalResult<bool> {
    let row: (bool,) =
        sqlx::query_as("SELECT EXISTS(SELECT 1 FROM users WHERE lower(username) = lower($1))")
            .bind(username)
            .fetch_one(pool)
            .await?;
    Ok(row.0)
}

pub async fn email_exists(pool: &PgPool, email: &str) -> DalResult<bool> {
    let row: (bool,) =
        sqlx::query_as("SELECT EXISTS(SELECT 1 FROM users WHERE lower(email) = lower($1))")
            .bind(email)
            .fetch_one(pool)
            .await?;
    Ok(row.0)
}

/// Store verification / password-reset token (hashed).
pub async fn upsert_verification_token(
    pool: impl sqlx::Executor<'_, Database = sqlx::Postgres>,
    user_id: Uuid,
    token_hash: &str,
    kind: &str,
    expires_at: chrono::DateTime<chrono::Utc>,
) -> DalResult<()> {
    sqlx::query(
        "INSERT INTO verification_tokens (id, user_id, token_hash, kind, expires_at)
         VALUES (gen_random_uuid(), $1, $2, $3, $4)
         ON CONFLICT (user_id, kind)
         DO UPDATE SET token_hash = $2, expires_at = $4, used_at = NULL, created_at = now()",
    )
    .bind(user_id)
    .bind(token_hash)
    .bind(kind)
    .bind(expires_at)
    .execute(pool)
    .await?;
    Ok(())
}

/// Consume a verification token. Returns the user_id if valid, None if missing/expired/used.
pub async fn consume_verification_token(
    pool: impl sqlx::Executor<'_, Database = sqlx::Postgres>,
    token_hash: &str,
    kind: &str,
) -> DalResult<Option<Uuid>> {
    let row: Option<(Uuid,)> = sqlx::query_as(
        "UPDATE verification_tokens
         SET used_at = now()
         WHERE token_hash = $1 AND kind = $2
           AND used_at IS NULL AND expires_at > now()
         RETURNING user_id",
    )
    .bind(token_hash)
    .bind(kind)
    .fetch_optional(pool)
    .await?;
    Ok(row.map(|(id,)| id))
}

/// Fail closed if a previously enrolled account unexpectedly receives password-only tokens.
pub async fn mfa_required(pool: &PgPool, user_id: Uuid) -> DalResult<bool> {
    Ok(
        sqlx::query_scalar::<_, bool>("SELECT required FROM user_mfa WHERE user_id=$1")
            .bind(user_id)
            .fetch_optional(pool)
            .await?
            .unwrap_or(false),
    )
}
pub async fn lock_mfa(connection: &mut sqlx::PgConnection, user_id: Uuid) -> DalResult<bool> {
    sqlx::query("INSERT INTO user_mfa(user_id) VALUES($1) ON CONFLICT DO NOTHING")
        .bind(user_id)
        .execute(&mut *connection)
        .await?;
    Ok(
        sqlx::query_scalar("SELECT required FROM user_mfa WHERE user_id=$1 FOR UPDATE")
            .bind(user_id)
            .fetch_one(connection)
            .await?,
    )
}
pub async fn set_mfa_required(
    connection: &mut sqlx::PgConnection,
    user_id: Uuid,
    required: bool,
) -> DalResult<()> {
    sqlx::query("UPDATE user_mfa SET required=$2 WHERE user_id=$1")
        .bind(user_id)
        .bind(required)
        .execute(connection)
        .await?;
    Ok(())
}

pub async fn lock_session(connection: &mut sqlx::PgConnection, user_id: Uuid) -> DalResult<User> {
    Ok(sqlx::query_as("SELECT * FROM users WHERE id=$1 FOR SHARE")
        .bind(user_id)
        .fetch_one(connection)
        .await?)
}
