use axum::{
    extract::{Path, State},
    http::StatusCode,
    routing::{delete, get, post},
    Json, Router,
};
use serde::Deserialize;
use serde_json::{json, Value};

use dal_common::error::DalError;
use dal_db::queries;

use crate::{extractors::AuthUser, state::AppState};

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/packages/{name}/owners", get(list_owners))
        .route("/packages/{name}/owners/invite", post(invite_owner))
        .route("/packages/{name}/owners/{username}", delete(remove_owner))
        .route("/packages/{name}/transfer", post(transfer_ownership))
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
    role:     Option<String>,
}

async fn invite_owner(
    State(state): State<AppState>,
    AuthUser(actor): AuthUser,
    Path(name): Path<String>,
    Json(body): Json<InviteOwnerBody>,
) -> Result<(StatusCode, Json<Value>), DalError> {
    let pkg = queries::packages::get_by_name(&state.db, &name)
        .await?
        .ok_or_else(|| DalError::PackageNotFound(name.clone()))?;

    if !queries::packages::is_owner(&state.db, pkg.id, actor.id).await? {
        return Err(DalError::Forbidden);
    }

    let invitee = queries::users::get_public_by_username(&state.db, &body.username)
        .await?
        .ok_or_else(|| DalError::UserNotFound(body.username.clone()))?;

    let role = body.role.as_deref().unwrap_or("collaborator");
    if !matches!(role, "owner" | "collaborator") {
        return Err(DalError::Validation(
            "role must be `owner` or `collaborator`".into(),
        ));
    }

    queries::packages::add_owner(&state.db, pkg.id, invitee.id, role, Some(actor.id)).await?;

    let _ = queries::audit::record(
        &state.db,
        Some(actor.id),
        "add_owner",
        Some(pkg.id),
        Some("package"),
        json!({ "package": pkg.name, "invitee": invitee.username, "role": role }),
    )
    .await;

    Ok((StatusCode::CREATED, Json(json!({ "message": "owner added" }))))
}

async fn remove_owner(
    State(state): State<AppState>,
    AuthUser(actor): AuthUser,
    Path((name, username)): Path<(String, String)>,
) -> Result<Json<Value>, DalError> {
    let pkg = queries::packages::get_by_name(&state.db, &name)
        .await?
        .ok_or_else(|| DalError::PackageNotFound(name.clone()))?;

    if !queries::packages::is_owner(&state.db, pkg.id, actor.id).await? {
        return Err(DalError::Forbidden);
    }

    let target = queries::users::get_public_by_username(&state.db, &username)
        .await?
        .ok_or_else(|| DalError::UserNotFound(username.clone()))?;

    // Prevent removing the last owner
    let owners = queries::packages::list_owners(&state.db, pkg.id).await?;
    if owners.len() <= 1 {
        return Err(DalError::Validation(
            "cannot remove the last owner of a package".into(),
        ));
    }

    queries::packages::remove_owner(&state.db, pkg.id, target.id).await?;

    let _ = queries::audit::record(
        &state.db,
        Some(actor.id),
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
    AuthUser(actor): AuthUser,
    Path(name): Path<String>,
    Json(body): Json<TransferBody>,
) -> Result<Json<Value>, DalError> {
    let pkg = queries::packages::get_by_name(&state.db, &name)
        .await?
        .ok_or_else(|| DalError::PackageNotFound(name.clone()))?;

    if !queries::packages::is_owner(&state.db, pkg.id, actor.id).await? {
        return Err(DalError::Forbidden);
    }

    let target = queries::users::get_public_by_username(&state.db, &body.to_username)
        .await?
        .ok_or_else(|| DalError::UserNotFound(body.to_username.clone()))?;

    queries::packages::transfer_ownership(&state.db, pkg.id, actor.id, target.id).await?;

    let _ = queries::audit::record(
        &state.db,
        Some(actor.id),
        "transfer_ownership",
        Some(pkg.id),
        Some("package"),
        json!({ "package": pkg.name, "from": actor.username, "to": target.username }),
    )
    .await;

    Ok(Json(json!({ "message": "ownership transferred" })))
}
