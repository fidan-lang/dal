use dal_common::Result as DalResult;
use sqlx::PgPool;
use uuid::Uuid;

pub async fn record(
    pool: &PgPool,
    actor_id: Option<Uuid>,
    action: &str,
    target_id: Option<Uuid>,
    target_type: Option<&str>,
    metadata: serde_json::Value,
) -> DalResult<()> {
    sqlx::query(
        "INSERT INTO audit_log (id, actor_id, action, target_id, target_type, metadata)
         VALUES (gen_random_uuid(), $1, $2, $3, $4, $5::jsonb)",
    )
    .bind(actor_id)
    .bind(action)
    .bind(target_id)
    .bind(target_type)
    .bind(metadata)
    .execute(pool)
    .await?;
    Ok(())
}
