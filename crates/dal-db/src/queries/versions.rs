use crate::models::PackageVersion;
use dal_common::Result as DalResult;
use sqlx::PgPool;
use uuid::Uuid;

pub struct NewPackageVersion<'a> {
    pub id: Uuid,
    pub package_id: Uuid,
    pub version: &'a str,
    pub checksum: &'a str,
    pub size_bytes: i64,
    pub s3_key: &'a str,
    pub readme: Option<&'a str>,
    pub manifest: serde_json::Value,
    pub published_by: Uuid,
}

pub async fn create(
    pool: &PgPool,
    new_version: NewPackageVersion<'_>,
) -> DalResult<PackageVersion> {
    let v = sqlx::query_as::<_, PackageVersion>(
        "INSERT INTO package_versions
             (id, package_id, version, checksum, size_bytes, s3_key,
              readme, manifest, published_by)
         VALUES ($1, $2, $3, $4, $5, $6, $7, $8::jsonb, $9)
         RETURNING *",
    )
    .bind(new_version.id)
    .bind(new_version.package_id)
    .bind(new_version.version)
    .bind(new_version.checksum)
    .bind(new_version.size_bytes)
    .bind(new_version.s3_key)
    .bind(new_version.readme)
    .bind(new_version.manifest)
    .bind(new_version.published_by)
    .fetch_one(pool)
    .await?;
    Ok(v)
}

pub async fn get(
    pool: &PgPool,
    package_id: Uuid,
    version: &str,
) -> DalResult<Option<PackageVersion>> {
    let v = sqlx::query_as::<_, PackageVersion>(
        "SELECT * FROM package_versions WHERE package_id = $1 AND version = $2",
    )
    .bind(package_id)
    .bind(version)
    .fetch_optional(pool)
    .await?;
    Ok(v)
}

pub async fn list_for_package(pool: &PgPool, package_id: Uuid) -> DalResult<Vec<PackageVersion>> {
    let versions = sqlx::query_as::<_, PackageVersion>(
        "SELECT * FROM package_versions WHERE package_id = $1 ORDER BY created_at DESC",
    )
    .bind(package_id)
    .fetch_all(pool)
    .await?;
    Ok(versions)
}

pub async fn latest(pool: &PgPool, package_id: Uuid) -> DalResult<Option<PackageVersion>> {
    let v = sqlx::query_as::<_, PackageVersion>(
        "SELECT * FROM package_versions
         WHERE package_id = $1 AND NOT yanked
         ORDER BY created_at DESC LIMIT 1",
    )
    .bind(package_id)
    .fetch_optional(pool)
    .await?;
    Ok(v)
}

pub async fn exists(pool: &PgPool, package_id: Uuid, version: &str) -> DalResult<bool> {
    let row: (bool,) = sqlx::query_as(
        "SELECT EXISTS(SELECT 1 FROM package_versions WHERE package_id = $1 AND version = $2)",
    )
    .bind(package_id)
    .bind(version)
    .fetch_one(pool)
    .await?;
    Ok(row.0)
}

pub async fn delete(pool: &PgPool, id: Uuid) -> DalResult<bool> {
    let res = sqlx::query("DELETE FROM package_versions WHERE id = $1")
        .bind(id)
        .execute(pool)
        .await?;
    Ok(res.rows_affected() > 0)
}

pub async fn yank(
    pool: &PgPool,
    package_id: Uuid,
    version: &str,
    reason: Option<&str>,
) -> DalResult<bool> {
    let res = sqlx::query(
        "UPDATE package_versions SET yanked = true, yank_reason = $3
         WHERE package_id = $1 AND version = $2 AND NOT yanked",
    )
    .bind(package_id)
    .bind(version)
    .bind(reason)
    .execute(pool)
    .await?;
    Ok(res.rows_affected() > 0)
}

pub async fn unyank(pool: &PgPool, package_id: Uuid, version: &str) -> DalResult<bool> {
    let res = sqlx::query(
        "UPDATE package_versions SET yanked = false, yank_reason = NULL
         WHERE package_id = $1 AND version = $2 AND yanked",
    )
    .bind(package_id)
    .bind(version)
    .execute(pool)
    .await?;
    Ok(res.rows_affected() > 0)
}

pub async fn increment_downloads(pool: &PgPool, id: Uuid) -> DalResult<()> {
    sqlx::query("UPDATE package_versions SET downloads = downloads + 1 WHERE id = $1")
        .bind(id)
        .execute(pool)
        .await?;
    Ok(())
}

/// Download counts grouped by day for the last N days (for charts).
pub async fn download_chart(
    pool: &PgPool,
    version_id: Uuid,
    days: i64,
) -> DalResult<Vec<(chrono::NaiveDate, i64)>> {
    let rows: Vec<(chrono::NaiveDate, i64)> = sqlx::query_as(
        "SELECT created_at::date AS day, COUNT(*) AS cnt
         FROM download_logs
         WHERE version_id = $1 AND created_at > now() - ($2 || ' days')::interval
         GROUP BY day ORDER BY day",
    )
    .bind(version_id)
    .bind(days)
    .fetch_all(pool)
    .await?;
    Ok(rows)
}

pub async fn record_download(
    pool: &PgPool,
    version_id: Uuid,
    ip_hash: &str,
    user_agent: Option<&str>,
) -> DalResult<()> {
    sqlx::query(
        "INSERT INTO download_logs (id, version_id, ip_hash, user_agent)
         VALUES (gen_random_uuid(), $1, $2, $3)",
    )
    .bind(version_id)
    .bind(ip_hash)
    .bind(user_agent)
    .execute(pool)
    .await?;
    Ok(())
}
