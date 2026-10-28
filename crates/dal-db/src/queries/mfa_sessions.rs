use chrono::{DateTime, Utc};
use dal_common::{Result, error::DalError};
use sqlx::{PgConnection, PgPool};
use uuid::Uuid;

pub async fn authorized(pool: &PgPool, user: Uuid, token_hash: &str) -> Result<bool> {
    Ok(
        sqlx::query_scalar("SELECT dal_security.mfa_authorized($1,$2)")
            .bind(user)
            .bind(token_hash)
            .fetch_one(pool)
            .await?,
    )
}
pub async fn valid_refresh(connection: &mut PgConnection, user: Uuid, hash: &str) -> Result<bool> {
    Ok(sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM mfa_sessions WHERE user_id=$1 AND refresh_hash=$2 AND expires_at>now())")
        .bind(user).bind(hash).fetch_one(connection).await?)
}
pub async fn create(connection: &mut PgConnection, user: Uuid, hash: &str) -> Result<()> {
    sqlx::query("DELETE FROM mfa_sessions WHERE user_id=$1 AND expires_at<=now()")
        .bind(user)
        .execute(&mut *connection)
        .await?;
    sqlx::query(
        "INSERT INTO mfa_sessions(user_id,refresh_hash) VALUES($1,$2) ON CONFLICT DO NOTHING",
    )
    .bind(user)
    .bind(hash)
    .execute(&mut *connection)
    .await?;
    if !valid_refresh(connection, user, hash).await? {
        return Err(DalError::Unauthorized);
    }
    Ok(())
}
pub async fn bind_token(
    connection: &mut PgConnection,
    refresh: &str,
    token: &str,
    expiry: DateTime<Utc>,
) -> Result<()> {
    sqlx::query("DELETE FROM mfa_id_tokens WHERE refresh_hash=$1 AND expires_at<=now()")
        .bind(refresh)
        .execute(&mut *connection)
        .await?;
    sqlx::query("INSERT INTO mfa_id_tokens(refresh_hash,token_hash,expires_at) VALUES($1,$2,$3) ON CONFLICT DO NOTHING")
        .bind(refresh).bind(token).bind(expiry).execute(connection).await?;
    Ok(())
}
