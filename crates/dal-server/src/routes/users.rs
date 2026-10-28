use axum::{
    Json, Router,
    extract::{Path, Query, State},
    routing::{get, patch},
};
use serde::Deserialize;
use serde_json::Value;

use dal_common::{error::DalError, pagination::Page, pagination::PageParams};
use dal_db::queries;

use crate::{extractors::AuthActor, responses::CurrentUserResponse, state::AppState};

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/users/{username}", get(get_user))
        .route("/users/{username}/packages", get(user_packages))
        .route("/users/me/profile", patch(update_profile))
}

async fn get_user(
    State(state): State<AppState>,
    Path(username): Path<String>,
) -> Result<Json<Value>, DalError> {
    let user = queries::users::get_public_by_username(&state.db, &username)
        .await?
        .ok_or_else(|| DalError::UserNotFound(username))?;
    Ok(Json(serde_json::to_value(&user).unwrap_or_default()))
}

async fn user_packages(
    State(state): State<AppState>,
    Path(username): Path<String>,
    Query(params): Query<PageParams>,
) -> Result<Json<Value>, DalError> {
    let user = queries::users::get_public_by_username(&state.db, &username)
        .await?
        .ok_or_else(|| DalError::UserNotFound(username))?;

    let (items, total) =
        queries::packages::list_by_owner(&state.db, user.id, params.limit(), params.offset())
            .await?;

    let page = Page::new(items, &params, total);
    Ok(Json(serde_json::to_value(&page).unwrap_or_default()))
}

#[derive(Deserialize)]
struct UpdateProfileBody {
    display_name: Option<String>,
    bio: Option<String>,
    website: Option<String>,
    avatar_url: Option<String>,
}

async fn update_profile(
    State(state): State<AppState>,
    actor: AuthActor,
    Json(body): Json<UpdateProfileBody>,
) -> Result<Json<Value>, DalError> {
    actor.require_scope(dal_auth::USER_WRITE_SCOPE)?;
    validate_profile_url(body.website.as_deref(), "website")?;
    validate_profile_url(body.avatar_url.as_deref(), "avatar_url")?;

    let mut tx = actor.transaction(&state.db).await?;
    let updated = queries::users::update_profile(
        &mut *tx,
        actor.user.id,
        body.display_name.as_deref(),
        body.bio.as_deref(),
        body.website.as_deref(),
        body.avatar_url.as_deref(),
    )
    .await?;
    tx.commit().await?;
    Ok(Json(
        serde_json::to_value(CurrentUserResponse::from(updated)).unwrap_or_default(),
    ))
}

fn validate_profile_url(value: Option<&str>, field: &str) -> Result<(), DalError> {
    let Some(value) = value.filter(|value| !value.is_empty()) else {
        return Ok(());
    };
    let parsed = url::Url::parse(value).ok();
    if !parsed.as_ref().is_some_and(|url| {
        matches!(url.scheme(), "http" | "https")
            && url.has_host()
            && url.username().is_empty()
            && url.password().is_none()
    }) {
        return Err(DalError::Validation(format!(
            "{field} must be an HTTP(S) URL without embedded credentials"
        )));
    }
    Ok(())
}

#[cfg(test)]
mod profile_url_tests {
    use super::*;
    #[test]
    fn public_profile_links_require_web_urls_without_credentials() {
        for value in [
            None,
            Some(""),
            Some("https://example.test/profile"),
            Some("http://localhost:8080/avatar.png"),
        ] {
            validate_profile_url(value, "website").unwrap();
        }
        for value in [
            "javascript:alert(1)",
            "data:text/html,unsafe",
            "not a URL",
            "//example.test",
            "https://user:password@example.test",
            "file:///etc/passwd",
        ] {
            assert_eq!(
                validate_profile_url(Some(value), "website")
                    .unwrap_err()
                    .status(),
                axum::http::StatusCode::UNPROCESSABLE_ENTITY
            );
        }
    }
}
