use crate::models::ApiToken;
use chrono::{DateTime, Utc};
use dal_common::Result as DalResult;
use uuid::Uuid;

pub async fn create(
    pool: impl sqlx::Executor<'_, Database = sqlx::Postgres>,
    user_id: Uuid,
    name: &str,
    token_hash: &str,
    prefix: &str,
    scopes: &[String],
    expires_at: Option<DateTime<Utc>>,
) -> DalResult<ApiToken> {
    let token = sqlx::query_as::<_, ApiToken>(
        "INSERT INTO api_tokens (id, user_id, name, token_hash, prefix, scopes, expires_at)
         VALUES (gen_random_uuid(), $1, $2, $3, $4, $5, $6)
         RETURNING *",
    )
    .bind(user_id)
    .bind(name)
    .bind(token_hash)
    .bind(prefix)
    .bind(scopes)
    .bind(expires_at)
    .fetch_one(pool)
    .await?;
    Ok(token)
}

pub async fn list_for_user(
    pool: impl sqlx::Executor<'_, Database = sqlx::Postgres>,
    user_id: Uuid,
) -> DalResult<Vec<ApiToken>> {
    let tokens = sqlx::query_as::<_, ApiToken>(
        "SELECT * FROM api_tokens WHERE user_id = $1 ORDER BY created_at DESC",
    )
    .bind(user_id)
    .fetch_all(pool)
    .await?;
    Ok(tokens)
}

pub async fn get_by_hash(
    pool: impl sqlx::Executor<'_, Database = sqlx::Postgres>,
    token_hash: &str,
) -> DalResult<Option<ApiToken>> {
    let token = sqlx::query_as::<_, ApiToken>("SELECT * FROM dal_security.auth_token($1)")
        .bind(token_hash)
        .fetch_optional(pool)
        .await?;
    Ok(token)
}

pub async fn touch(
    pool: impl sqlx::Executor<'_, Database = sqlx::Postgres>,
    hash: &str,
) -> DalResult<()> {
    sqlx::query("SELECT dal_security.touch_token($1)")
        .bind(hash)
        .execute(pool)
        .await?;
    Ok(())
}

pub async fn delete(
    pool: impl sqlx::Executor<'_, Database = sqlx::Postgres>,
    id: Uuid,
    user_id: Uuid,
) -> DalResult<bool> {
    let res = sqlx::query("DELETE FROM api_tokens WHERE id = $1 AND user_id = $2")
        .bind(id)
        .bind(user_id)
        .execute(pool)
        .await?;
    Ok(res.rows_affected() > 0)
}
