mod common;

use axum::http::Method;
use dal_auth::{DEFAULT_API_TOKEN_SCOPES, OWNER_SCOPE, USER_WRITE_SCOPE, api_token::hash_token};
use dal_db::{connect, queries};
use serde_json::json;
use uuid::Uuid;

fn raw_token() -> String {
    format!("dal_{}{}", Uuid::new_v4().simple(), Uuid::new_v4().simple())
}

async fn seed_member_token(
    db: &sqlx::PgPool,
    user_id: Uuid,
    name: &str,
    scopes: &[String],
) -> String {
    let token = raw_token();
    let prefix = token[..8].to_string();

    queries::tokens::create(
        db,
        user_id,
        name,
        &hash_token(&token),
        &prefix,
        scopes,
        None,
    )
    .await
    .unwrap();

    token
}

#[tokio::test]
async fn collaborator_cannot_manage_ownership() {
    let app = common::TestApp::spawn().await;
    dotenvy::from_filename(".env.test").ok();
    let database_url = std::env::var("TEST_DATABASE_URL")
        .unwrap_or_else(|_| "postgres://dal_test:test@localhost:5433/dal_test".to_string());
    let db = connect(&database_url).await.unwrap();

    let owner_id = Uuid::new_v4();
    let collaborator_id = Uuid::new_v4();
    let invitee_id = Uuid::new_v4();
    let package_id = Uuid::new_v4();
    let package_name = format!("owners-perms-{}", Uuid::new_v4().simple());
    let owner_username = format!("owner-perms-{}", Uuid::new_v4().simple());
    let collaborator_username = format!("collab-perms-{}", Uuid::new_v4().simple());
    let invitee_username = format!("invitee-perms-{}", Uuid::new_v4().simple());

    queries::users::create(
        &db,
        owner_id,
        &owner_username,
        &format!("{owner_username}@example.test"),
        &Uuid::new_v4().to_string(),
        Some("Owner User"),
    )
    .await
    .unwrap();
    queries::users::create(
        &db,
        collaborator_id,
        &collaborator_username,
        &format!("{collaborator_username}@example.test"),
        &Uuid::new_v4().to_string(),
        None,
    )
    .await
    .unwrap();
    queries::users::create(
        &db,
        invitee_id,
        &invitee_username,
        &format!("{invitee_username}@example.test"),
        &Uuid::new_v4().to_string(),
        None,
    )
    .await
    .unwrap();

    queries::packages::create(
        &db,
        queries::packages::NewPackage {
            id: package_id,
            name: &package_name,
            description: Some("permission test package"),
            repository: None,
            homepage: None,
            license: None,
            keywords: &[],
            categories: &[],
        },
    )
    .await
    .unwrap();
    queries::packages::add_owner(&db, package_id, owner_id, "owner", None)
        .await
        .unwrap();
    queries::packages::add_owner(
        &db,
        package_id,
        collaborator_id,
        "collaborator",
        Some(owner_id),
    )
    .await
    .unwrap();

    let collaborator_token = seed_member_token(
        &db,
        collaborator_id,
        "collab-perms-token",
        &DEFAULT_API_TOKEN_SCOPES
            .iter()
            .map(|scope| scope.to_string())
            .collect::<Vec<_>>(),
    )
    .await;

    let invite_res = app
        .request_json(
            Method::POST,
            &format!("/packages/{package_name}/owners/invite"),
            json!({ "username": invitee_username, "role": "owner" }),
            Some(&collaborator_token),
        )
        .await;
    let transfer_res = app
        .request_json(
            Method::POST,
            &format!("/packages/{package_name}/transfer"),
            json!({ "to_username": owner_username }),
            Some(&collaborator_token),
        )
        .await;

    assert_eq!(invite_res.status().as_u16(), 403);
    assert_eq!(transfer_res.status().as_u16(), 403);
}

#[tokio::test]
async fn collaborator_does_not_count_as_last_owner() {
    let app = common::TestApp::spawn().await;
    dotenvy::from_filename(".env.test").ok();
    let database_url = std::env::var("TEST_DATABASE_URL")
        .unwrap_or_else(|_| "postgres://dal_test:test@localhost:5433/dal_test".to_string());
    let db = connect(&database_url).await.unwrap();

    let owner_id = Uuid::new_v4();
    let collaborator_id = Uuid::new_v4();
    let package_id = Uuid::new_v4();
    let package_name = format!("last-owner-{}", Uuid::new_v4().simple());
    let owner_username = format!("owner-last-{}", Uuid::new_v4().simple());
    let collaborator_username = format!("collab-last-{}", Uuid::new_v4().simple());

    queries::users::create(
        &db,
        owner_id,
        &owner_username,
        &format!("{owner_username}@example.test"),
        &Uuid::new_v4().to_string(),
        None,
    )
    .await
    .unwrap();
    queries::users::create(
        &db,
        collaborator_id,
        &collaborator_username,
        &format!("{collaborator_username}@example.test"),
        &Uuid::new_v4().to_string(),
        None,
    )
    .await
    .unwrap();

    queries::packages::create(
        &db,
        queries::packages::NewPackage {
            id: package_id,
            name: &package_name,
            description: Some("last owner test package"),
            repository: None,
            homepage: None,
            license: None,
            keywords: &[],
            categories: &[],
        },
    )
    .await
    .unwrap();
    queries::packages::add_owner(&db, package_id, owner_id, "owner", None)
        .await
        .unwrap();
    queries::packages::add_owner(
        &db,
        package_id,
        collaborator_id,
        "collaborator",
        Some(owner_id),
    )
    .await
    .unwrap();

    let owner_token = seed_member_token(
        &db,
        owner_id,
        "owner-last-token",
        &[OWNER_SCOPE.to_string()],
    )
    .await;

    let remove_res = app
        .request_json(
            Method::DELETE,
            &format!("/packages/{package_name}/owners/{owner_username}"),
            json!({}),
            Some(&owner_token),
        )
        .await;
    let (status, body) = common::TestApp::unpack(remove_res).await;

    assert_eq!(status.as_u16(), 422);
    assert_eq!(
        body["error"]["message"],
        "validation error: cannot remove the last owner of a package"
    );
}

#[tokio::test]
async fn owner_invites_are_pending_until_accepted() {
    let app = common::TestApp::spawn().await;
    dotenvy::from_filename(".env.test").ok();
    let database_url = std::env::var("TEST_DATABASE_URL")
        .unwrap_or_else(|_| "postgres://dal_test:test@localhost:5433/dal_test".to_string());
    let db = connect(&database_url).await.unwrap();

    let owner_id = Uuid::new_v4();
    let invitee_id = Uuid::new_v4();
    let package_id = Uuid::new_v4();
    let package_name = format!("invite-pending-{}", Uuid::new_v4().simple());
    let owner_username = format!("owner-invite-{}", Uuid::new_v4().simple());
    let invitee_username = format!("invitee-pending-{}", Uuid::new_v4().simple());

    queries::users::create(
        &db,
        owner_id,
        &owner_username,
        &format!("{owner_username}@example.test"),
        &Uuid::new_v4().to_string(),
        None,
    )
    .await
    .unwrap();
    queries::users::create(
        &db,
        invitee_id,
        &invitee_username,
        &format!("{invitee_username}@example.test"),
        &Uuid::new_v4().to_string(),
        None,
    )
    .await
    .unwrap();
    queries::packages::create(
        &db,
        queries::packages::NewPackage {
            id: package_id,
            name: &package_name,
            description: Some("invite lifecycle package"),
            repository: None,
            homepage: None,
            license: None,
            keywords: &[],
            categories: &[],
        },
    )
    .await
    .unwrap();
    queries::packages::add_owner(&db, package_id, owner_id, "owner", None)
        .await
        .unwrap();

    let owner_token = seed_member_token(
        &db,
        owner_id,
        "owner-invite-token",
        &[OWNER_SCOPE.to_string()],
    )
    .await;
    let invitee_token = seed_member_token(
        &db,
        invitee_id,
        "invitee-dashboard-token",
        &DEFAULT_API_TOKEN_SCOPES
            .iter()
            .map(|scope| scope.to_string())
            .collect::<Vec<_>>(),
    )
    .await;

    let invite_res = app
        .request_json(
            Method::POST,
            &format!("/packages/{package_name}/owners/invite"),
            json!({ "username": invitee_username, "role": "collaborator" }),
            Some(&owner_token),
        )
        .await;
    let (invite_status, invite_body) = common::TestApp::unpack(invite_res).await;

    assert_eq!(invite_status.as_u16(), 201);
    assert_eq!(invite_body["message"], "invite sent");
    assert_eq!(invite_body["invite"]["package_name"], package_name);

    assert!(
        !queries::packages::is_member(&db, package_id, invitee_id)
            .await
            .unwrap()
    );

    let invites_res = app
        .request_json(
            Method::GET,
            "/owners/invites",
            json!({}),
            Some(&invitee_token),
        )
        .await;
    let (invites_status, invites_body) = common::TestApp::unpack(invites_res).await;

    assert_eq!(invites_status.as_u16(), 200);
    let invites = invites_body.as_array().unwrap();
    assert_eq!(invites.len(), 1);
    let invite_id = invites[0]["id"].as_str().unwrap().to_string();

    let accept_res = app
        .request_json(
            Method::POST,
            &format!("/owners/invites/{invite_id}/accept"),
            json!({}),
            Some(&invitee_token),
        )
        .await;
    let (accept_status, accept_body) = common::TestApp::unpack(accept_res).await;

    assert_eq!(accept_status.as_u16(), 200);
    assert_eq!(accept_body["message"], "invite accepted");
    assert!(
        queries::packages::is_member(&db, package_id, invitee_id)
            .await
            .unwrap()
    );
}

#[tokio::test]
async fn owner_token_without_owner_scope_cannot_manage_ownership() {
    let app = common::TestApp::spawn().await;
    dotenvy::from_filename(".env.test").ok();
    let database_url = std::env::var("TEST_DATABASE_URL")
        .unwrap_or_else(|_| "postgres://dal_test:test@localhost:5433/dal_test".to_string());
    let db = connect(&database_url).await.unwrap();

    let owner_id = Uuid::new_v4();
    let invitee_id = Uuid::new_v4();
    let package_id = Uuid::new_v4();
    let package_name = format!("owner-scope-{}", Uuid::new_v4().simple());
    let owner_username = format!("owner-scope-{}", Uuid::new_v4().simple());
    let invitee_username = format!("invitee-scope-{}", Uuid::new_v4().simple());

    queries::users::create(
        &db,
        owner_id,
        &owner_username,
        &format!("{owner_username}@example.test"),
        &Uuid::new_v4().to_string(),
        None,
    )
    .await
    .unwrap();
    queries::users::create(
        &db,
        invitee_id,
        &invitee_username,
        &format!("{invitee_username}@example.test"),
        &Uuid::new_v4().to_string(),
        None,
    )
    .await
    .unwrap();
    queries::packages::create(
        &db,
        queries::packages::NewPackage {
            id: package_id,
            name: &package_name,
            description: Some("owner scope package"),
            repository: None,
            homepage: None,
            license: None,
            keywords: &[],
            categories: &[],
        },
    )
    .await
    .unwrap();
    queries::packages::add_owner(&db, package_id, owner_id, "owner", None)
        .await
        .unwrap();

    let limited_owner_token = seed_member_token(
        &db,
        owner_id,
        "owner-no-owner-scope-token",
        &DEFAULT_API_TOKEN_SCOPES
            .iter()
            .map(|scope| scope.to_string())
            .collect::<Vec<_>>(),
    )
    .await;

    let invite_res = app
        .request_json(
            Method::POST,
            &format!("/packages/{package_name}/owners/invite"),
            json!({ "username": invitee_username, "role": "collaborator" }),
            Some(&limited_owner_token),
        )
        .await;

    assert_eq!(invite_res.status().as_u16(), 403);
}

#[tokio::test]
async fn profile_updates_require_user_write_scope_for_api_tokens() {
    let app = common::TestApp::spawn().await;
    dotenvy::from_filename(".env.test").ok();
    let database_url = std::env::var("TEST_DATABASE_URL")
        .unwrap_or_else(|_| "postgres://dal_test:test@localhost:5433/dal_test".to_string());
    let db = connect(&database_url).await.unwrap();

    let user_id = Uuid::new_v4();
    let username = format!("profile-scope-{}", Uuid::new_v4().simple());

    queries::users::create(
        &db,
        user_id,
        &username,
        &format!("{username}@example.test"),
        &Uuid::new_v4().to_string(),
        None,
    )
    .await
    .unwrap();

    let limited_token = seed_member_token(
        &db,
        user_id,
        "profile-no-write-scope",
        &DEFAULT_API_TOKEN_SCOPES
            .iter()
            .map(|scope| scope.to_string())
            .collect::<Vec<_>>(),
    )
    .await;
    let full_token = seed_member_token(
        &db,
        user_id,
        "profile-write-scope",
        &[USER_WRITE_SCOPE.to_string()],
    )
    .await;

    let forbidden = app
        .request_json(
            Method::PATCH,
            "/users/me/profile",
            json!({ "display_name": "Blocked" }),
            Some(&limited_token),
        )
        .await;
    assert_eq!(forbidden.status().as_u16(), 403);

    let allowed = app
        .request_json(
            Method::PATCH,
            "/users/me/profile",
            json!({ "display_name": "Allowed" }),
            Some(&full_token),
        )
        .await;
    let (status, body) = common::TestApp::unpack(allowed).await;

    assert_eq!(status.as_u16(), 200);
    assert_eq!(body["display_name"], "Allowed");
}
