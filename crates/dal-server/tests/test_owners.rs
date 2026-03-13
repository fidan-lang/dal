mod common;

use axum::http::Method;
use dal_auth::api_token::hash_token;
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
) -> String {
    let token = raw_token();
    let prefix = token[..8].to_string();

    queries::tokens::create(db, user_id, name, &hash_token(&token), &prefix, None)
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
        package_id,
        &package_name,
        Some("permission test package"),
        None,
        None,
        None,
        &[],
        &[],
    )
    .await
    .unwrap();
    queries::packages::add_owner(&db, package_id, owner_id, "owner", None)
        .await
        .unwrap();
    queries::packages::add_owner(&db, package_id, collaborator_id, "collaborator", Some(owner_id))
        .await
        .unwrap();

    let collaborator_token = seed_member_token(&db, collaborator_id, "collab-perms-token").await;

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
        package_id,
        &package_name,
        Some("last owner test package"),
        None,
        None,
        None,
        &[],
        &[],
    )
    .await
    .unwrap();
    queries::packages::add_owner(&db, package_id, owner_id, "owner", None)
        .await
        .unwrap();
    queries::packages::add_owner(&db, package_id, collaborator_id, "collaborator", Some(owner_id))
        .await
        .unwrap();

    let owner_token = seed_member_token(&db, owner_id, "owner-last-token").await;

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
