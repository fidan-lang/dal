pub mod models;
pub mod queries;

pub use sqlx::PgPool;

/// Create a connection pool from `DATABASE_URL` environment variable.
pub async fn connect(database_url: &str) -> Result<PgPool, sqlx::Error> {
    let pool = sqlx::postgres::PgPoolOptions::new()
        .max_connections(20)
        .connect(database_url)
        .await?;
    Ok(pool)
}

/// Run pending migrations from the `migrations/` directory at the workspace root.
pub async fn migrate(pool: &PgPool) -> Result<(), sqlx::migrate::MigrateError> {
    sqlx::migrate!("../../migrations").run(pool).await
}
