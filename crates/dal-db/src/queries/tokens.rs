use crate::models::ApiToken;
use dal_common::Result as DalResult;
use sqlx::PgPool;
use uuid::Uuid;
use chrono::{DateTime, Utc};

pub async fn create(
    pool: &PgPool,
    user_id: Uuid,
    name: &str,
    token_hash: &str,
    prefix: &str,
    expires_at: Option<DateTime<Utc>>,
) -> DalResult<ApiToken> {
    let token = sqlx::query_as::<_, ApiToken>(
        "INSERT INTO api_tokens (id, user_id, name, token_hash, prefix, expires_at)
         VALUES (gen_random_uuid(), $1, $2, $3, $4, $5)
         RETURNING *",
    )
    .bind(user_id)
    .bind(name)
    .bind(token_hash)
    .bind(prefix)
    .bind(expires_at)
    .fetch_one(pool)
    .await?;
    Ok(token)
}

pub async fn list_for_user(pool: &PgPool, user_id: Uuid) -> DalResult<Vec<ApiToken>> {
    let tokens = sqlx::query_as::<_, ApiToken>(
        "SELECT * FROM api_tokens WHERE user_id = $1 ORDER BY created_at DESC",
    )
    .bind(user_id)
    .fetch_all(pool)
    .await?;
    Ok(tokens)
}

pub async fn get_by_hash(pool: &PgPool, token_hash: &str) -> DalResult<Option<ApiToken>> {
    let token = sqlx::query_as::<_, ApiToken>(
        "SELECT * FROM api_tokens
         WHERE token_hash = $1
           AND (expires_at IS NULL OR expires_at > now())",
    )
    .bind(token_hash)
    .fetch_optional(pool)
    .await?;
    Ok(token)
}

pub async fn touch(pool: &PgPool, id: Uuid) -> DalResult<()> {
    sqlx::query("UPDATE api_tokens SET last_used_at = now() WHERE id = $1")
        .bind(id)
        .execute(pool)
        .await?;
    Ok(())
}

pub async fn delete(pool: &PgPool, id: Uuid, user_id: Uuid) -> DalResult<bool> {
    let res = sqlx::query(
        "DELETE FROM api_tokens WHERE id = $1 AND user_id = $2",
    )
    .bind(id)
    .bind(user_id)
    .execute(pool)
    .await?;
    Ok(res.rows_affected() > 0)
}
