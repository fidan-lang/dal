//! Run only against docker-compose.test.yml's disposable PostgreSQL.
use sqlx::{PgPool, postgres::PgPoolOptions};

async fn owner() -> PgPool {
    let address = std::env::var("TEST_DATABASE_URL")
        .unwrap_or_else(|_| "postgres://dal_test:test@127.0.0.1:5433/dal_test".into());
    let url = url::Url::parse(&address).unwrap();
    assert!(matches!(url.host_str(), Some("127.0.0.1" | "localhost")));
    assert_eq!(url.port(), Some(5433));
    assert_eq!(url.path(), "/dal_test");
    let db = PgPoolOptions::new()
        .max_connections(2)
        .connect(&address)
        .await
        .unwrap();
    dal_db::migrate(&db).await.unwrap();
    db
}

#[tokio::test]
#[ignore = "requires disposable PostgreSQL; scripts/test.sh --with-cognito"]
async fn runtime_roles_are_real_non_owner_non_bypass_identities() {
    let db = owner().await;
    let count: i64 = sqlx::query_scalar("SELECT count(*) FROM pg_roles WHERE rolname IN ('dal_api','dal_worker') AND NOT rolsuper AND NOT rolbypassrls AND NOT rolcreaterole AND NOT rolcreatedb AND NOT rolinherit").fetch_one(&db).await.unwrap();
    assert_eq!(
        count, 2,
        "dedicated restricted runtime roles must be provisioned by migration"
    );
    let memberships:i64=sqlx::query_scalar("SELECT count(*) FROM pg_auth_members m JOIN pg_roles r ON m.member=r.oid WHERE r.rolname IN ('dal_api','dal_worker')").fetch_one(&db).await.unwrap();
    assert_eq!(
        memberships, 0,
        "runtime must not inherit owner/neon_superuser or SET ROLE authority"
    );
}

async fn runtime(role: &str) -> PgPool {
    let db = owner().await;
    sqlx::raw_sql(
        "ALTER ROLE dal_api PASSWORD 'test-api'; ALTER ROLE dal_worker PASSWORD 'test-worker';",
    )
    .execute(&db)
    .await
    .unwrap();
    db.close().await;
    PgPoolOptions::new()
        .max_connections(1)
        .connect(&format!(
            "postgres://{role}:test-{}@127.0.0.1:5433/dal_test",
            if role == "dal_api" { "api" } else { "worker" }
        ))
        .await
        .unwrap()
}
struct Fixture {
    owner: PgPool,
    api: PgPool,
    users: [uuid::Uuid; 3],
    packages: [uuid::Uuid; 2],
    token: uuid::Uuid,
}
async fn fixture() -> Fixture {
    let db = owner().await;
    let users = [
        uuid::Uuid::new_v4(),
        uuid::Uuid::new_v4(),
        uuid::Uuid::new_v4(),
    ];
    for (i, id) in users.iter().enumerate() {
        sqlx::query("INSERT INTO users(id,username,email,cognito_sub,email_verified,is_admin) VALUES($1,$2,$3,$2,true,$4)")
            .bind(id).bind(id.to_string()).bind(format!("{id}@fixture.test")).bind(i==2).execute(&db).await.unwrap();
        sqlx::query("INSERT INTO api_tokens(user_id,name,token_hash,prefix,scopes) VALUES($1,'fixture',$2,'fixture',ARRAY['publish:update','yank'])")
            .bind(id).bind(format!("{id:064x}",id=id.as_u128())).execute(&db).await.unwrap();
        sqlx::query("INSERT INTO user_mfa(user_id,required) VALUES($1,true)")
            .bind(id)
            .execute(&db)
            .await
            .unwrap();
        sqlx::query("INSERT INTO mfa_sessions(user_id,refresh_hash) VALUES($1,$2)")
            .bind(id)
            .bind(format!("{id:064x}", id = id.as_u128()))
            .execute(&db)
            .await
            .unwrap();
        sqlx::query("INSERT INTO mfa_id_tokens(refresh_hash,token_hash,expires_at) VALUES($1,$1,now()+interval '1 hour')").bind(format!("{id:064x}",id=id.as_u128())).execute(&db).await.unwrap();
    }
    let packages = [uuid::Uuid::new_v4(), uuid::Uuid::new_v4()];
    for (i, id) in packages.iter().enumerate() {
        sqlx::query("INSERT INTO packages(id,name,created_by) VALUES($1,$2,$3)")
            .bind(id)
            .bind(id.to_string())
            .bind(users[i])
            .execute(&db)
            .await
            .unwrap();
        sqlx::query("INSERT INTO package_owners(package_id,user_id) VALUES($1,$2)")
            .bind(id)
            .bind(users[i])
            .execute(&db)
            .await
            .unwrap();
    }
    let token = sqlx::query_scalar("SELECT id FROM api_tokens WHERE user_id=$1")
        .bind(users[2])
        .fetch_one(&db)
        .await
        .unwrap();
    Fixture {
        owner: db,
        api: runtime("dal_api").await,
        users,
        packages,
        token,
    }
}

#[tokio::test]
#[ignore = "requires disposable PostgreSQL; scripts/test.sh --with-cognito"]
async fn sensitive_rows_are_isolated_and_pool_context_never_leaks() {
    let f = fixture().await;
    dal_db::verify_runtime(&f.api, "dal_api").await.unwrap();
    assert!(dal_db::verify_runtime(&f.owner, "dal_api").await.is_err());
    for table in [
        "users",
        "api_tokens",
        "owner_invites",
        "user_mfa",
        "mfa_sessions",
        "mfa_id_tokens",
    ] {
        let count: i64 = sqlx::query_scalar(&format!("SELECT count(*) FROM {table}"))
            .fetch_one(&f.api)
            .await
            .unwrap();
        assert_eq!(count, 0, "missing actor must hide {table}");
    }
    for id in &f.users[..2] {
        let mut tx = dal_db::actor_transaction(&f.api, *id, None).await.unwrap();
        let visible: Vec<uuid::Uuid> = sqlx::query_scalar("SELECT id FROM users")
            .fetch_all(&mut *tx)
            .await
            .unwrap();
        assert_eq!(visible, vec![*id]);
        let credentials: Vec<uuid::Uuid> = sqlx::query_scalar("SELECT user_id FROM api_tokens")
            .fetch_all(&mut *tx)
            .await
            .unwrap();
        assert_eq!(credentials, vec![*id]);
        let proofs: Vec<uuid::Uuid> = sqlx::query_scalar("SELECT user_id FROM mfa_sessions")
            .fetch_all(&mut *tx)
            .await
            .unwrap();
        assert_eq!(proofs, vec![*id]);
        let changed = sqlx::query("UPDATE users SET display_name='unauthorized' WHERE id<>$1")
            .bind(id)
            .execute(&mut *tx)
            .await
            .unwrap();
        assert_eq!(changed.rows_affected(), 0);
        assert_eq!(
            sqlx::query("DELETE FROM api_tokens WHERE user_id<>$1")
                .bind(id)
                .execute(&mut *tx)
                .await
                .unwrap()
                .rows_affected(),
            0
        );
        tx.commit().await.unwrap();
        let count: i64 = sqlx::query_scalar("SELECT count(*) FROM api_tokens")
            .fetch_one(&f.api)
            .await
            .unwrap();
        assert_eq!(count, 0);
    }
    let mut tx = dal_db::actor_transaction(&f.api, f.users[0], None)
        .await
        .unwrap();
    assert!(
        sqlx::query(
            "INSERT INTO api_tokens(user_id,name,token_hash,prefix) VALUES($1,'bad','bad','bad')"
        )
        .bind(f.users[1])
        .execute(&mut *tx)
        .await
        .is_err()
    );
    tx.rollback().await.unwrap();
    let mut tx = dal_db::actor_transaction(&f.api, f.users[0], None)
        .await
        .unwrap();
    assert!(
        sqlx::query("INSERT INTO mfa_sessions(user_id,refresh_hash) VALUES($1,repeat('f',64))")
            .bind(f.users[1])
            .execute(&mut *tx)
            .await
            .is_err()
    );
    drop(tx);
    assert_eq!(
        sqlx::query_scalar::<_, i64>("SELECT count(*) FROM mfa_sessions")
            .fetch_one(&f.api)
            .await
            .unwrap(),
        0,
        "cancelled/dropped transactions must reset pooled actor context"
    );
    for value in ["not-a-uuid", "", "00000000-0000-0000-0000-000000000000"] {
        let mut tx = f.api.begin().await.unwrap();
        sqlx::query(
            "SELECT set_config('dal.actor',$1,true),set_config('dal.auth_kind','session',true)",
        )
        .bind(value)
        .execute(&mut *tx)
        .await
        .unwrap();
        let count: i64 = sqlx::query_scalar("SELECT count(*) FROM users")
            .fetch_one(&mut *tx)
            .await
            .unwrap();
        assert_eq!(count, 0);
        tx.rollback().await.unwrap();
    }
    for statement in [
        "ALTER TABLE users DISABLE ROW LEVEL SECURITY",
        "ALTER ROLE dal_api BYPASSRLS",
        "SET ROLE dal_test",
        "CREATE TABLE public.rls_escape(id int)",
        "CREATE SCHEMA rls_escape",
        "CREATE TEMP TABLE rls_temp(id int)",
        "CREATE FUNCTION dal_security.escape() RETURNS int LANGUAGE sql AS 'SELECT 1'",
        "UPDATE users SET is_admin=true",
        "SELECT * FROM verification_tokens",
    ] {
        assert!(
            sqlx::query(statement).execute(&f.api).await.is_err(),
            "forbidden statement: {statement}"
        );
    }
    let mut admin = dal_db::actor_transaction(&f.api, f.users[2], None)
        .await
        .unwrap();
    assert!(
        sqlx::query_scalar::<_, bool>("SELECT dal_security.admin()")
            .fetch_one(&mut *admin)
            .await
            .unwrap()
    );
    admin.rollback().await.unwrap();
    let mut api_token = dal_db::actor_transaction(&f.api, f.users[2], Some(f.token))
        .await
        .unwrap();
    assert!(
        !sqlx::query_scalar::<_, bool>("SELECT dal_security.admin()")
            .fetch_one(&mut *api_token)
            .await
            .unwrap()
    );
    assert_eq!(
        sqlx::query_scalar::<_, i64>("SELECT count(*) FROM mfa_sessions")
            .fetch_one(&mut *api_token)
            .await
            .unwrap(),
        0
    );
    assert_eq!(
        sqlx::query_scalar::<_, i64>("SELECT count(*) FROM api_tokens")
            .fetch_one(&mut *api_token)
            .await
            .unwrap(),
        0
    );
    api_token.rollback().await.unwrap();
    let mut wrong_owner = dal_db::actor_transaction(&f.api, f.users[0], Some(f.token))
        .await
        .unwrap();
    assert!(
        !sqlx::query_scalar::<_, bool>("SELECT dal_security.allowed()")
            .fetch_one(&mut *wrong_owner)
            .await
            .unwrap()
    );
    wrong_owner.rollback().await.unwrap();
    let own_token: uuid::Uuid = sqlx::query_scalar("SELECT id FROM api_tokens WHERE user_id=$1")
        .bind(f.users[0])
        .fetch_one(&f.owner)
        .await
        .unwrap();
    let mut scoped = dal_db::actor_transaction(&f.api, f.users[0], Some(own_token))
        .await
        .unwrap();
    assert_eq!(
        sqlx::query("UPDATE users SET display_name='wrong-scope'")
            .execute(&mut *scoped)
            .await
            .unwrap()
            .rows_affected(),
        0,
        "publish/yank scopes do not grant user:write"
    );
    scoped.rollback().await.unwrap();
    let mut no_rls = dal_db::actor_transaction(&f.api, f.users[0], None)
        .await
        .unwrap();
    sqlx::query("SET LOCAL row_security=off")
        .execute(&mut *no_rls)
        .await
        .unwrap();
    assert!(
        sqlx::query("SELECT * FROM api_tokens")
            .execute(&mut *no_rls)
            .await
            .is_err(),
        "row_security=off must error, never bypass policies"
    );
    no_rls.rollback().await.unwrap();
}

#[tokio::test]
#[ignore = "requires disposable PostgreSQL; scripts/test.sh --with-cognito"]
async fn ownership_invitations_and_publication_reinforce_rust_authorization() {
    use dal_db::queries;
    let f = fixture().await;
    let mut tx = dal_db::actor_transaction(&f.api, f.users[0], None)
        .await
        .unwrap();
    assert_eq!(
        sqlx::query("UPDATE packages SET description='cross-user' WHERE id=$1")
            .bind(f.packages[1])
            .execute(&mut *tx)
            .await
            .unwrap()
            .rows_affected(),
        0
    );
    tx.rollback().await.unwrap();
    let mut tx = dal_db::actor_transaction(&f.api, f.users[0], None)
        .await
        .unwrap();
    assert!(
        queries::packages::add_owner(&mut *tx, f.packages[1], f.users[0], "owner", None)
            .await
            .is_err()
    );
    tx.rollback().await.unwrap();
    let mut tx = dal_db::actor_transaction(&f.api, f.users[0], None)
        .await
        .unwrap();
    let invite = queries::owner_invites::upsert(
        &mut *tx,
        f.packages[0],
        f.users[1],
        f.users[0],
        "fixture-hash",
        "collaborator",
        chrono::Utc::now() + chrono::Duration::days(1),
    )
    .await
    .unwrap();
    tx.commit().await.unwrap();
    let mut tx = dal_db::actor_transaction(&f.api, f.users[1], None)
        .await
        .unwrap();
    assert!(
        sqlx::query("UPDATE owner_invites SET role='owner' WHERE id=$1")
            .bind(invite.id)
            .execute(&mut *tx)
            .await
            .is_err()
    );
    tx.rollback().await.unwrap();
    assert!(
        queries::owner_invites::accept(&f.api, invite.id, f.users[2])
            .await
            .unwrap()
            .is_none()
    );
    let accepted = queries::owner_invites::accept(&f.api, invite.id, f.users[1])
        .await
        .unwrap()
        .unwrap();
    assert_eq!(accepted.role, "collaborator");
    assert!(
        queries::packages::is_member(&f.api, f.packages[0], f.users[1])
            .await
            .unwrap()
    );
    let mut tx = dal_db::actor_transaction(&f.api, f.users[0], None)
        .await
        .unwrap();
    let new = uuid::Uuid::new_v4();
    sqlx::query("INSERT INTO packages(id,name) VALUES($1,$2)")
        .bind(new)
        .bind(new.to_string())
        .execute(&mut *tx)
        .await
        .unwrap();
    queries::packages::add_owner(&mut *tx, new, f.users[0], "owner", None)
        .await
        .unwrap();
    sqlx::query("INSERT INTO package_versions(package_id,version,checksum,size_bytes,s3_key,published_by) VALUES($1,'0.1.0','fixture',1,'fixture',$2)").bind(new).bind(f.users[0]).execute(&mut *tx).await.unwrap();
    tx.commit().await.unwrap();
    assert!(
        queries::packages::get_by_id(&f.api, new)
            .await
            .unwrap()
            .is_some(),
        "anonymous package discovery remains public"
    );
}

#[tokio::test]
#[ignore = "requires disposable PostgreSQL; scripts/test.sh --with-cognito"]
async fn service_capabilities_and_worker_have_narrow_permissions() {
    use dal_db::queries;
    let f = fixture().await;
    let worker = runtime("dal_worker").await;
    dal_db::verify_runtime(&worker, "dal_worker").await.unwrap();
    assert_eq!(
        sqlx::query_scalar::<_, i64>("SELECT count(*) FROM users")
            .fetch_one(&worker)
            .await
            .unwrap(),
        0
    );
    for table in [
        "api_tokens",
        "verification_tokens",
        "user_mfa",
        "mfa_sessions",
        "mfa_id_tokens",
        "owner_invites",
    ] {
        assert!(
            sqlx::query(&format!("SELECT * FROM {table}"))
                .execute(&worker)
                .await
                .is_err()
        );
    }
    let id = uuid::Uuid::new_v4();
    queries::users::create(
        &f.api,
        id,
        &id.to_string(),
        &format!("{id}@fixture.test"),
        &id.to_string(),
        None,
    )
    .await
    .unwrap();
    let hash = format!("{id:064x}", id = id.as_u128());
    queries::users::upsert_verification_token(
        &f.api,
        id,
        &hash,
        "email_verify",
        chrono::Utc::now() + chrono::Duration::hours(1),
    )
    .await
    .unwrap();
    let mut tx = f.api.begin().await.unwrap();
    assert_eq!(
        queries::users::consume_verification_token(&mut *tx, &hash, "email_verify")
            .await
            .unwrap(),
        Some(id)
    );
    tx.commit().await.unwrap();
    let mut verified = dal_db::actor_transaction(&f.api, id, None).await.unwrap();
    assert!(
        queries::users::get_by_id(&mut *verified, id)
            .await
            .unwrap()
            .unwrap()
            .email_verified
    );
    verified.rollback().await.unwrap();
    assert!(
        queries::users::consume_verification_token(&f.api, &hash, "email_verify")
            .await
            .unwrap()
            .is_none()
    );
    let stale = uuid::Uuid::new_v4();
    queries::users::create(
        &f.owner,
        stale,
        &stale.to_string(),
        &format!("{stale}@fixture.test"),
        &stale.to_string(),
        None,
    )
    .await
    .unwrap();
    sqlx::query("UPDATE users SET created_at=now()-interval '30 days' WHERE id=$1")
        .bind(stale)
        .execute(&f.owner)
        .await
        .unwrap();
    let cutoff = chrono::Utc::now() - chrono::Duration::days(7);
    let mut tx = worker.begin().await.unwrap();
    assert!(
        queries::users::lock_stale_unverified(&mut *tx, f.users[0], cutoff)
            .await
            .unwrap()
            .is_none()
    );
    assert!(
        queries::users::lock_stale_unverified(&mut *tx, stale, cutoff)
            .await
            .unwrap()
            .is_some()
    );
    assert!(
        queries::users::delete_unverified(&mut *tx, stale)
            .await
            .unwrap()
    );
    tx.commit().await.unwrap();
    assert!(
        sqlx::query("SELECT * FROM dal_security.auth_user('id',$1)")
            .bind(f.users[0].to_string())
            .execute(&worker)
            .await
            .is_err()
    );
}

#[test]
fn migration_cli_never_falls_back_to_runtime_credentials() {
    let scratch = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../target")
        .join(format!("migration-cli-{}", uuid::Uuid::new_v4()));
    std::fs::create_dir_all(&scratch).unwrap();
    let mut command = std::process::Command::new(env!("CARGO_BIN_EXE_dal-server"));
    command
        .arg("--migrate")
        .current_dir(&scratch)
        .env_clear()
        .env(
            "DATABASE_URL",
            "postgres://unusable-runtime@127.0.0.1:1/never-connect",
        );
    if let Some(root) = std::env::var_os("SystemRoot") {
        command.env("SystemRoot", root);
    }
    let output = command.output().unwrap();
    std::fs::remove_dir(scratch).unwrap();
    assert!(!output.status.success());
    assert!(output.stdout.is_empty());
    assert!(
        String::from_utf8_lossy(&output.stderr).contains("DAL_MIGRATION_DATABASE_URL required")
    );
}
