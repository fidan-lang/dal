use crate::models::{Package, PackageOwner, PackageSummary};
use dal_common::Result as DalResult;
use sqlx::PgPool;
use uuid::Uuid;

pub struct NewPackage<'a> {
    pub id: Uuid,
    pub name: &'a str,
    pub description: Option<&'a str>,
    pub repository: Option<&'a str>,
    pub homepage: Option<&'a str>,
    pub license: Option<&'a str>,
    pub keywords: &'a [String],
    pub categories: &'a [String],
}

pub struct PackageMetadataUpdate<'a> {
    pub id: Uuid,
    pub description: Option<&'a str>,
    pub repository: Option<&'a str>,
    pub homepage: Option<&'a str>,
    pub license: Option<&'a str>,
    pub readme: Option<&'a str>,
    pub keywords: &'a [String],
    pub categories: &'a [String],
}

pub async fn create(pool: &PgPool, new_package: NewPackage<'_>) -> DalResult<Package> {
    let pkg = sqlx::query_as::<_, Package>(
        "INSERT INTO packages
             (id, name, description, repository, homepage, license, keywords, categories)
         VALUES ($1, $2, $3, $4, $5, $6, $7::jsonb, $8::jsonb)
         RETURNING *",
    )
    .bind(new_package.id)
    .bind(new_package.name)
    .bind(new_package.description)
    .bind(new_package.repository)
    .bind(new_package.homepage)
    .bind(new_package.license)
    .bind(serde_json::to_value(new_package.keywords).unwrap())
    .bind(serde_json::to_value(new_package.categories).unwrap())
    .fetch_one(pool)
    .await?;
    Ok(pkg)
}

pub async fn get_by_name(pool: &PgPool, name: &str) -> DalResult<Option<Package>> {
    let pkg = sqlx::query_as::<_, Package>("SELECT * FROM packages WHERE lower(name) = lower($1)")
        .bind(name)
        .fetch_optional(pool)
        .await?;
    Ok(pkg)
}

pub async fn get_by_id(pool: &PgPool, id: Uuid) -> DalResult<Option<Package>> {
    let pkg = sqlx::query_as::<_, Package>("SELECT * FROM packages WHERE id = $1")
        .bind(id)
        .fetch_optional(pool)
        .await?;
    Ok(pkg)
}

pub async fn name_exists(pool: &PgPool, name: &str) -> DalResult<bool> {
    let row: (bool,) =
        sqlx::query_as("SELECT EXISTS(SELECT 1 FROM packages WHERE lower(name) = lower($1))")
            .bind(name)
            .fetch_one(pool)
            .await?;
    Ok(row.0)
}

pub async fn update_metadata(
    pool: &PgPool,
    update: PackageMetadataUpdate<'_>,
) -> DalResult<Package> {
    let pkg = sqlx::query_as::<_, Package>(
        "UPDATE packages SET
             description = COALESCE($2, description),
             repository  = COALESCE($3, repository),
             homepage    = COALESCE($4, homepage),
             license     = COALESCE($5, license),
             readme      = COALESCE($6, readme),
             keywords    = $7::jsonb,
             categories  = $8::jsonb,
             updated_at  = now()
         WHERE id = $1
         RETURNING *",
    )
    .bind(update.id)
    .bind(update.description)
    .bind(update.repository)
    .bind(update.homepage)
    .bind(update.license)
    .bind(update.readme)
    .bind(serde_json::to_value(update.keywords).unwrap())
    .bind(serde_json::to_value(update.categories).unwrap())
    .fetch_one(pool)
    .await?;
    Ok(pkg)
}

pub async fn increment_downloads(pool: &PgPool, id: Uuid) -> DalResult<()> {
    sqlx::query("UPDATE packages SET downloads = downloads + 1, updated_at = now() WHERE id = $1")
        .bind(id)
        .execute(pool)
        .await?;
    Ok(())
}

/// Full-text + keyword search. Returns summary rows.
pub async fn search(
    pool: &PgPool,
    q: &str,
    limit: i64,
    offset: i64,
) -> DalResult<(Vec<PackageSummary>, i64)> {
    // Use PostgreSQL full-text search on name + description
    let total: (i64,) = sqlx::query_as(
        "SELECT COUNT(*) FROM packages
         WHERE to_tsvector('english', name || ' ' || COALESCE(description,''))
               @@ plainto_tsquery('english', $1)",
    )
    .bind(q)
    .fetch_one(pool)
    .await?;

    let items = sqlx::query_as::<_, PackageSummary>(
        "SELECT p.id, p.name, p.description, p.license, p.downloads,
                (SELECT v.version FROM package_versions v
                 WHERE v.package_id = p.id AND NOT v.yanked
                 ORDER BY v.created_at DESC LIMIT 1) AS latest_version,
                p.updated_at
         FROM packages p
         WHERE to_tsvector('english', p.name || ' ' || COALESCE(p.description,''))
               @@ plainto_tsquery('english', $1)
         ORDER BY p.downloads DESC
         LIMIT $2 OFFSET $3",
    )
    .bind(q)
    .bind(limit)
    .bind(offset)
    .fetch_all(pool)
    .await?;

    Ok((items, total.0))
}

/// List all packages, sorted by download count descending.
pub async fn list(pool: &PgPool, limit: i64, offset: i64) -> DalResult<(Vec<PackageSummary>, i64)> {
    let total: (i64,) = sqlx::query_as("SELECT COUNT(*) FROM packages")
        .fetch_one(pool)
        .await?;

    let items = sqlx::query_as::<_, PackageSummary>(
        "SELECT p.id, p.name, p.description, p.license, p.downloads,
                (SELECT v.version FROM package_versions v
                 WHERE v.package_id = p.id AND NOT v.yanked
                 ORDER BY v.created_at DESC LIMIT 1) AS latest_version,
                p.updated_at
         FROM packages p
         ORDER BY p.downloads DESC
         LIMIT $1 OFFSET $2",
    )
    .bind(limit)
    .bind(offset)
    .fetch_all(pool)
    .await?;

    Ok((items, total.0))
}

// ── Ownership ─────────────────────────────────────────────────────────────────

pub async fn add_owner(
    pool: &PgPool,
    package_id: Uuid,
    user_id: Uuid,
    role: &str,
    invited_by: Option<Uuid>,
) -> DalResult<()> {
    sqlx::query(
        "INSERT INTO package_owners (package_id, user_id, role, invited_by)
         VALUES ($1, $2, $3, $4)
         ON CONFLICT (package_id, user_id) DO NOTHING",
    )
    .bind(package_id)
    .bind(user_id)
    .bind(role)
    .bind(invited_by)
    .execute(pool)
    .await?;
    Ok(())
}

pub async fn remove_owner(pool: &PgPool, package_id: Uuid, user_id: Uuid) -> DalResult<bool> {
    let res = sqlx::query("DELETE FROM package_owners WHERE package_id = $1 AND user_id = $2")
        .bind(package_id)
        .bind(user_id)
        .execute(pool)
        .await?;
    Ok(res.rows_affected() > 0)
}

pub async fn list_owners(pool: &PgPool, package_id: Uuid) -> DalResult<Vec<PackageOwner>> {
    let owners = sqlx::query_as::<_, PackageOwner>(
        "SELECT * FROM package_owners WHERE package_id = $1 ORDER BY created_at ASC",
    )
    .bind(package_id)
    .fetch_all(pool)
    .await?;
    Ok(owners)
}

pub async fn is_member(pool: &PgPool, package_id: Uuid, user_id: Uuid) -> DalResult<bool> {
    let row: (bool,) = sqlx::query_as(
        "SELECT EXISTS(SELECT 1 FROM package_owners WHERE package_id = $1 AND user_id = $2)",
    )
    .bind(package_id)
    .bind(user_id)
    .fetch_one(pool)
    .await?;
    Ok(row.0)
}

pub async fn is_owner(pool: &PgPool, package_id: Uuid, user_id: Uuid) -> DalResult<bool> {
    let row: (bool,) = sqlx::query_as(
        "SELECT EXISTS(
            SELECT 1
            FROM package_owners
            WHERE package_id = $1 AND user_id = $2 AND role = 'owner'
        )",
    )
    .bind(package_id)
    .bind(user_id)
    .fetch_one(pool)
    .await?;
    Ok(row.0)
}

/// List all packages owned by a user (for user profile page).
pub async fn list_by_owner(
    pool: &PgPool,
    user_id: Uuid,
    limit: i64,
    offset: i64,
) -> DalResult<(Vec<PackageSummary>, i64)> {
    let total: (i64,) = sqlx::query_as("SELECT COUNT(*) FROM package_owners WHERE user_id = $1")
        .bind(user_id)
        .fetch_one(pool)
        .await?;

    let items = sqlx::query_as::<_, PackageSummary>(
        "SELECT p.id, p.name, p.description, p.license, p.downloads,
                (SELECT v.version FROM package_versions v
                 WHERE v.package_id = p.id AND NOT v.yanked
                 ORDER BY v.created_at DESC LIMIT 1) AS latest_version,
                p.updated_at
         FROM packages p
         JOIN package_owners po ON po.package_id = p.id
         WHERE po.user_id = $1
         ORDER BY p.updated_at DESC
         LIMIT $2 OFFSET $3",
    )
    .bind(user_id)
    .bind(limit)
    .bind(offset)
    .fetch_all(pool)
    .await?;

    Ok((items, total.0))
}

/// Transfer primary ownership to another user.
pub async fn transfer_ownership(
    pool: &PgPool,
    package_id: Uuid,
    from_user_id: Uuid,
    to_user_id: Uuid,
) -> DalResult<()> {
    let mut tx = pool.begin().await?;

    // Ensure target is already an owner/collaborator — upsert as owner
    sqlx::query(
        "INSERT INTO package_owners (package_id, user_id, role, invited_by)
         VALUES ($1, $2, 'owner', $3)
         ON CONFLICT (package_id, user_id) DO UPDATE SET role = 'owner'",
    )
    .bind(package_id)
    .bind(to_user_id)
    .bind(from_user_id)
    .execute(&mut *tx)
    .await?;

    // Downgrade old owner to collaborator
    sqlx::query(
        "UPDATE package_owners SET role = 'collaborator'
         WHERE package_id = $1 AND user_id = $2",
    )
    .bind(package_id)
    .bind(from_user_id)
    .execute(&mut *tx)
    .await?;

    tx.commit().await?;
    Ok(())
}
