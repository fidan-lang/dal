use axum::{
    Json, Router,
    extract::{Path, State},
    http::StatusCode,
    routing::{delete, get, post},
};
use chrono::{Duration, Utc};
use serde::Deserialize;
use serde_json::{Value, json};
use uuid::Uuid;

use dal_auth::api_token::hash_token;
use dal_common::error::DalError;
use dal_db::{models::OwnershipInvite, queries};

use crate::{
    extractors::{AuthActor, AuthUser},
    state::AppState,
};

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/packages/{name}/owners", get(list_owners))
        .route("/packages/{name}/owners/invite", post(invite_owner))
        .route("/packages/{name}/owners/{username}", delete(remove_owner))
        .route("/packages/{name}/transfer", post(transfer_ownership))
        .route("/owners/invites", get(list_pending_invites))
        .route("/owners/invites/{id}/accept", post(accept_invite))
        .route("/owners/invites/{id}/decline", post(decline_invite))
}

async fn list_owners(
    State(state): State<AppState>,
    Path(name): Path<String>,
) -> Result<Json<Value>, DalError> {
    let pkg = queries::packages::get_by_name(&state.db, &name)
        .await?
        .ok_or_else(|| DalError::PackageNotFound(name))?;
    let owners = queries::packages::list_owners(&state.db, pkg.id).await?;
    let mut enriched = Vec::with_capacity(owners.len());

    for owner in owners {
        let user = queries::users::get_by_id(&state.db, owner.user_id)
            .await?
            .ok_or_else(|| DalError::UserNotFound(owner.user_id.to_string()))?;

        enriched.push(json!({
            "user_id": owner.user_id,
            "username": user.username,
            "display_name": user.display_name,
            "role": owner.role,
            "added_at": owner.created_at,
        }));
    }

    Ok(Json(Value::Array(enriched)))
}

#[derive(Deserialize)]
struct InviteOwnerBody {
    username: String,
    role: Option<String>,
}

async fn invite_owner(
    State(state): State<AppState>,
    actor: AuthActor,
    Path(name): Path<String>,
    Json(body): Json<InviteOwnerBody>,
) -> Result<(StatusCode, Json<Value>), DalError> {
    actor.require_scope(dal_auth::OWNER_SCOPE)?;

    let pkg = queries::packages::get_by_name(&state.db, &name)
        .await?
        .ok_or_else(|| DalError::PackageNotFound(name.clone()))?;

    if !queries::packages::is_owner(&state.db, pkg.id, actor.user.id).await? {
        return Err(DalError::Forbidden);
    }

    let invitee = queries::users::get_by_username(&state.db, &body.username.trim().to_lowercase())
        .await?
        .ok_or_else(|| DalError::UserNotFound(body.username.clone()))?;

    if invitee.id == actor.user.id {
        return Err(DalError::Validation("you cannot invite yourself".into()));
    }

    if queries::packages::is_member(&state.db, pkg.id, invitee.id).await? {
        return Err(DalError::Validation(
            "user is already a member of this package".into(),
        ));
    }

    let role = body.role.as_deref().unwrap_or("collaborator");
    if !matches!(role, "owner" | "collaborator") {
        return Err(DalError::Validation(
            "role must be `owner` or `collaborator`".into(),
        ));
    }

    let raw_token = format!("oi_{}", hex::encode(Uuid::new_v4().as_bytes()));
    let token_hash = hash_token(&raw_token);
    let expires_at = Utc::now() + Duration::days(7);
    let invite = queries::owner_invites::upsert(
        &state.db,
        pkg.id,
        invitee.id,
        actor.user.id,
        &token_hash,
        role,
        expires_at,
    )
    .await?;

    let _ = enqueue_job(
        &state,
        json!({
            "kind": "owner_invite",
            "package": pkg.name,
            "role": role,
            "email": invitee.email,
            "username": invitee.username,
            "inviter_username": actor.user.username,
        }),
    )
    .await;

    let _ = queries::audit::record(
        &state.db,
        Some(actor.user.id),
        "invite_owner",
        Some(pkg.id),
        Some("package"),
        json!({ "package": pkg.name, "invitee": invitee.username, "role": role }),
    )
    .await;

    Ok((
        StatusCode::CREATED,
        Json(json!({
            "message": "invite sent",
            "invite": serialize_invite(&state, &invite).await?,
        })),
    ))
}

async fn list_pending_invites(
    State(state): State<AppState>,
    AuthUser(user): AuthUser,
) -> Result<Json<Value>, DalError> {
    let invites = queries::owner_invites::list_pending_for_user(&state.db, user.id).await?;
    let mut enriched = Vec::with_capacity(invites.len());

    for invite in invites {
        enriched.push(serialize_invite(&state, &invite).await?);
    }

    Ok(Json(Value::Array(enriched)))
}

async fn accept_invite(
    State(state): State<AppState>,
    AuthUser(user): AuthUser,
    Path(id): Path<Uuid>,
) -> Result<Json<Value>, DalError> {
    let pending = queries::owner_invites::get_pending_for_user(&state.db, id, user.id)
        .await?
        .ok_or_else(|| DalError::Validation("invite not found or expired".into()))?;
    let accepted = queries::owner_invites::accept(&state.db, id, user.id)
        .await?
        .ok_or_else(|| DalError::Validation("invite not found or expired".into()))?;
    let pkg = queries::packages::get_by_id(&state.db, accepted.package_id)
        .await?
        .ok_or_else(|| DalError::PackageNotFound(accepted.package_id.to_string()))?;

    let _ = queries::audit::record(
        &state.db,
        Some(user.id),
        "accept_owner_invite",
        Some(pkg.id),
        Some("package"),
        json!({ "package": pkg.name, "role": pending.role }),
    )
    .await;

    Ok(Json(json!({
        "message": "invite accepted",
        "invite": serialize_invite(&state, &accepted).await?,
    })))
}

async fn decline_invite(
    State(state): State<AppState>,
    AuthUser(user): AuthUser,
    Path(id): Path<Uuid>,
) -> Result<Json<Value>, DalError> {
    let pending = queries::owner_invites::get_pending_for_user(&state.db, id, user.id)
        .await?
        .ok_or_else(|| DalError::Validation("invite not found or expired".into()))?;
    let pkg = queries::packages::get_by_id(&state.db, pending.package_id)
        .await?
        .ok_or_else(|| DalError::PackageNotFound(pending.package_id.to_string()))?;

    if !queries::owner_invites::decline(&state.db, id, user.id).await? {
        return Err(DalError::Validation("invite not found or expired".into()));
    }

    let _ = queries::audit::record(
        &state.db,
        Some(user.id),
        "decline_owner_invite",
        Some(pkg.id),
        Some("package"),
        json!({ "package": pkg.name, "role": pending.role }),
    )
    .await;

    Ok(Json(json!({ "message": "invite declined" })))
}

async fn remove_owner(
    State(state): State<AppState>,
    actor: AuthActor,
    Path((name, username)): Path<(String, String)>,
) -> Result<Json<Value>, DalError> {
    actor.require_scope(dal_auth::OWNER_SCOPE)?;

    let pkg = queries::packages::get_by_name(&state.db, &name)
        .await?
        .ok_or_else(|| DalError::PackageNotFound(name.clone()))?;

    if !queries::packages::is_owner(&state.db, pkg.id, actor.user.id).await? {
        return Err(DalError::Forbidden);
    }

    let target = queries::users::get_public_by_username(&state.db, &username)
        .await?
        .ok_or_else(|| DalError::UserNotFound(username.clone()))?;

    let owners = queries::packages::list_owners(&state.db, pkg.id).await?;
    let owner_count = owners.iter().filter(|owner| owner.role == "owner").count();
    let target_owner = owners.iter().find(|owner| owner.user_id == target.id);

    if target_owner.is_none() {
        return Err(DalError::Validation(
            "user is not a member of this package".into(),
        ));
    }

    if target_owner.is_some_and(|owner| owner.role == "owner") && owner_count <= 1 {
        return Err(DalError::Validation(
            "cannot remove the last owner of a package".into(),
        ));
    }

    let removed = queries::packages::remove_owner(&state.db, pkg.id, target.id).await?;
    if !removed {
        return Err(DalError::Validation(
            "user is not a member of this package".into(),
        ));
    }

    let _ = queries::audit::record(
        &state.db,
        Some(actor.user.id),
        "remove_owner",
        Some(pkg.id),
        Some("package"),
        json!({ "package": pkg.name, "removed": username }),
    )
    .await;

    Ok(Json(json!({ "message": "owner removed" })))
}

#[derive(Deserialize)]
struct TransferBody {
    to_username: String,
}

async fn transfer_ownership(
    State(state): State<AppState>,
    actor: AuthActor,
    Path(name): Path<String>,
    Json(body): Json<TransferBody>,
) -> Result<Json<Value>, DalError> {
    actor.require_scope(dal_auth::OWNER_SCOPE)?;

    let pkg = queries::packages::get_by_name(&state.db, &name)
        .await?
        .ok_or_else(|| DalError::PackageNotFound(name.clone()))?;

    if !queries::packages::is_owner(&state.db, pkg.id, actor.user.id).await? {
        return Err(DalError::Forbidden);
    }

    let target = queries::users::get_public_by_username(&state.db, &body.to_username)
        .await?
        .ok_or_else(|| DalError::UserNotFound(body.to_username.clone()))?;

    queries::packages::transfer_ownership(&state.db, pkg.id, actor.user.id, target.id).await?;

    let _ = queries::audit::record(
        &state.db,
        Some(actor.user.id),
        "transfer_ownership",
        Some(pkg.id),
        Some("package"),
        json!({ "package": pkg.name, "from": actor.user.username, "to": target.username }),
    )
    .await;

    Ok(Json(json!({ "message": "ownership transferred" })))
}

async fn serialize_invite(state: &AppState, invite: &OwnershipInvite) -> Result<Value, DalError> {
    let pkg = queries::packages::get_by_id(&state.db, invite.package_id)
        .await?
        .ok_or_else(|| DalError::PackageNotFound(invite.package_id.to_string()))?;
    let inviter = queries::users::get_by_id(&state.db, invite.inviter_id)
        .await?
        .ok_or_else(|| DalError::UserNotFound(invite.inviter_id.to_string()))?;

    Ok(json!({
        "id": invite.id,
        "package_id": invite.package_id,
        "package_name": pkg.name,
        "package_description": pkg.description,
        "inviter_username": inviter.username,
        "inviter_display_name": inviter.display_name,
        "role": invite.role,
        "created_at": invite.created_at,
        "expires_at": invite.expires_at,
        "accepted_at": invite.accepted_at,
        "declined_at": invite.declined_at,
    }))
}

async fn enqueue_job(state: &AppState, payload: Value) -> Result<(), DalError> {
    let kind = payload["kind"].as_str().unwrap_or("unknown");
    let body = serde_json::to_string(&payload).unwrap();
    let dedup_id = format!("{kind}:{}", Uuid::new_v4());

    state
        .sqs_client
        .send_message()
        .queue_url(&state.sqs_url)
        .message_body(body)
        .message_group_id("email")
        .message_deduplication_id(dedup_id)
        .send()
        .await
        .map(|_| ())
        .map_err(|error| DalError::Sqs(error.to_string()))
}
