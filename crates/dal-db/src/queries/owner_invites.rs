use crate::models::OwnershipInvite;
use dal_common::Result as DalResult;
use sqlx::{PgPool, Row};
use uuid::Uuid;

pub async fn upsert(
    pool: &PgPool,
    package_id: Uuid,
    invitee_id: Uuid,
    inviter_id: Uuid,
    token_hash: &str,
    role: &str,
    expires_at: chrono::DateTime<chrono::Utc>,
) -> DalResult<OwnershipInvite> {
    let invite = sqlx::query_as::<_, OwnershipInvite>(
        "INSERT INTO owner_invites
             (id, package_id, invitee_id, inviter_id, token_hash, role, expires_at)
         VALUES (gen_random_uuid(), $1, $2, $3, $4, $5, $6)
         ON CONFLICT (package_id, invitee_id)
         DO UPDATE SET
             inviter_id = EXCLUDED.inviter_id,
             token_hash = EXCLUDED.token_hash,
             role = EXCLUDED.role,
             accepted_at = NULL,
             declined_at = NULL,
             expires_at = EXCLUDED.expires_at,
             created_at = now()
         RETURNING *",
    )
    .bind(package_id)
    .bind(invitee_id)
    .bind(inviter_id)
    .bind(token_hash)
    .bind(role)
    .bind(expires_at)
    .fetch_one(pool)
    .await?;
    Ok(invite)
}

pub async fn list_pending_for_user(
    pool: &PgPool,
    invitee_id: Uuid,
) -> DalResult<Vec<OwnershipInvite>> {
    let invites = sqlx::query_as::<_, OwnershipInvite>(
        "SELECT *
         FROM owner_invites
         WHERE invitee_id = $1
           AND accepted_at IS NULL
           AND declined_at IS NULL
           AND expires_at > now()
         ORDER BY created_at DESC",
    )
    .bind(invitee_id)
    .fetch_all(pool)
    .await?;
    Ok(invites)
}

pub async fn get_pending_for_user(
    pool: &PgPool,
    id: Uuid,
    invitee_id: Uuid,
) -> DalResult<Option<OwnershipInvite>> {
    let invite = sqlx::query_as::<_, OwnershipInvite>(
        "SELECT *
         FROM owner_invites
         WHERE id = $1
           AND invitee_id = $2
           AND accepted_at IS NULL
           AND declined_at IS NULL
           AND expires_at > now()",
    )
    .bind(id)
    .bind(invitee_id)
    .fetch_optional(pool)
    .await?;
    Ok(invite)
}

pub async fn accept(
    pool: &PgPool,
    id: Uuid,
    invitee_id: Uuid,
) -> DalResult<Option<OwnershipInvite>> {
    let mut tx = pool.begin().await?;

    let invite = sqlx::query_as::<_, OwnershipInvite>(
        "SELECT *
         FROM owner_invites
         WHERE id = $1
           AND invitee_id = $2
           AND accepted_at IS NULL
           AND declined_at IS NULL
           AND expires_at > now()
         FOR UPDATE",
    )
    .bind(id)
    .bind(invitee_id)
    .fetch_optional(&mut *tx)
    .await?;

    let Some(invite) = invite else {
        tx.commit().await?;
        return Ok(None);
    };

    sqlx::query(
        "INSERT INTO package_owners (package_id, user_id, role, invited_by)
         VALUES ($1, $2, $3, $4)
         ON CONFLICT (package_id, user_id)
         DO UPDATE SET role = EXCLUDED.role, invited_by = EXCLUDED.invited_by",
    )
    .bind(invite.package_id)
    .bind(invite.invitee_id)
    .bind(&invite.role)
    .bind(invite.inviter_id)
    .execute(&mut *tx)
    .await?;

    let accepted = sqlx::query_as::<_, OwnershipInvite>(
        "UPDATE owner_invites
         SET accepted_at = now(), declined_at = NULL
         WHERE id = $1
         RETURNING *",
    )
    .bind(id)
    .fetch_one(&mut *tx)
    .await?;

    tx.commit().await?;
    Ok(Some(accepted))
}

pub async fn decline(pool: &PgPool, id: Uuid, invitee_id: Uuid) -> DalResult<bool> {
    let result = sqlx::query(
        "UPDATE owner_invites
         SET declined_at = now()
         WHERE id = $1
           AND invitee_id = $2
           AND accepted_at IS NULL
           AND declined_at IS NULL
           AND expires_at > now()",
    )
    .bind(id)
    .bind(invitee_id)
    .execute(pool)
    .await?;
    Ok(result.rows_affected() > 0)
}

pub async fn pending_count_for_user(pool: &PgPool, invitee_id: Uuid) -> DalResult<i64> {
    let row = sqlx::query(
        "SELECT COUNT(*) AS count
         FROM owner_invites
         WHERE invitee_id = $1
           AND accepted_at IS NULL
           AND declined_at IS NULL
           AND expires_at > now()",
    )
    .bind(invitee_id)
    .fetch_one(pool)
    .await?;
    Ok(row.get::<i64, _>("count"))
}
