pub mod models;
pub mod queries;

pub use sqlx::PgPool;

/// Create a connection pool from `DATABASE_URL` environment variable.
pub async fn connect(database_url: &str) -> Result<PgPool, sqlx::Error> {
    let pool = sqlx::postgres::PgPoolOptions::new()
        .max_connections(20)
        .acquire_timeout(std::time::Duration::from_secs(10))
        .idle_timeout(std::time::Duration::from_secs(300))
        .connect(database_url)
        .await?;
    Ok(pool)
}

/// Run pending migrations from the `migrations/` directory at the workspace root.
pub async fn migrate(pool: &PgPool) -> Result<(), sqlx::migrate::MigrateError> {
    sqlx::migrate!("../../migrations").run(pool).await
}

/// Bind only an identity already verified by DAL authentication. This is not a
/// SQL-injection boundary: PostgreSQL custom settings can be changed by SQL.
/// Transaction ownership ensures commit, rollback, cancellation and pool reuse
/// cannot retain another request's identity.
pub async fn actor_transaction(
    pool: &PgPool,
    actor: uuid::Uuid,
    api_token: Option<uuid::Uuid>,
) -> Result<sqlx::Transaction<'_, sqlx::Postgres>, sqlx::Error> {
    let mut tx = pool.begin().await?;
    bind_actor(&mut tx, actor, api_token).await?;
    Ok(tx)
}

pub async fn bind_actor(
    connection: &mut sqlx::PgConnection,
    actor: uuid::Uuid,
    api_token: Option<uuid::Uuid>,
) -> Result<(), sqlx::Error> {
    sqlx::query("SELECT set_config('dal.actor',$1,true), set_config('dal.auth_kind',$2,true), set_config('dal.api_token',$3,true)")
        .bind(actor.to_string()).bind(if api_token.is_some() { "api" } else { "session" })
        .bind(api_token.map(|id| id.to_string()).unwrap_or_default()).execute(connection).await?;
    Ok(())
}

/// Runtime never migrates or accepts a privileged/owner connection, even if a
/// deployment accidentally supplies the old owner DATABASE_URL.
pub async fn verify_runtime(pool: &PgPool, expected: &str) -> anyhow::Result<()> {
    anyhow::ensure!(
        matches!(expected, "dal_api" | "dal_worker"),
        "invalid runtime identity"
    );
    let safe: bool = sqlx::query_scalar("SELECT current_user=$1 AND NOT r.rolsuper AND NOT r.rolbypassrls AND NOT r.rolcreatedb AND NOT r.rolcreaterole AND NOT r.rolreplication AND NOT r.rolinherit AND NOT EXISTS(SELECT FROM pg_auth_members m WHERE m.member=r.oid) AND NOT EXISTS(SELECT FROM pg_class c JOIN pg_namespace n ON n.oid=c.relnamespace WHERE c.relowner=r.oid AND n.nspname IN ('public','dal_security')) AND NOT has_database_privilege(current_user,current_database(),'CREATE') AND NOT has_database_privilege(current_user,current_database(),'TEMP') AND NOT has_schema_privilege(current_user,'public','CREATE') AND NOT has_schema_privilege(current_user,'dal_security','CREATE') FROM pg_roles r WHERE r.rolname=current_user")
        .bind(expected).fetch_one(pool).await?;
    anyhow::ensure!(
        safe,
        "restricted non-owner {expected} database identity required; migrations use --migrate separately"
    );
    const PROTECTED_TABLES: &[&str] = &[
        "users",
        "verification_tokens",
        "api_tokens",
        "packages",
        "package_versions",
        "package_owners",
        "owner_invites",
        "download_logs",
        "audit_log",
        "user_mfa",
        "mfa_sessions",
        "mfa_id_tokens",
    ];
    let protected: i64 = sqlx::query_scalar("SELECT count(*) FROM pg_class c JOIN pg_namespace n ON n.oid=c.relnamespace WHERE n.nspname='public' AND c.relname=ANY($1) AND c.relrowsecurity AND c.relforcerowsecurity")
        .bind(PROTECTED_TABLES).fetch_one(pool).await?;
    anyhow::ensure!(
        protected == PROTECTED_TABLES.len() as i64,
        "owner must apply runtime RLS migration before startup"
    );
    Ok(())
}
