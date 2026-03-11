use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

// ── Users ─────────────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize, sqlx::FromRow)]
pub struct User {
    pub id:              Uuid,
    pub username:        String,
    pub email:           String,
    pub cognito_sub:     String,
    pub display_name:    Option<String>,
    pub avatar_url:      Option<String>,
    pub bio:             Option<String>,
    pub website:         Option<String>,
    pub is_admin:        bool,
    pub email_verified:  bool,
    pub created_at:      DateTime<Utc>,
    pub updated_at:      DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize, sqlx::FromRow)]
pub struct UserPublic {
    pub id:           Uuid,
    pub username:     String,
    pub display_name: Option<String>,
    pub avatar_url:   Option<String>,
    pub bio:          Option<String>,
    pub website:      Option<String>,
    pub created_at:   DateTime<Utc>,
}

// ── Email verification / password-reset tokens ────────────────────────────────

#[derive(Debug, Clone, sqlx::FromRow)]
pub struct VerificationToken {
    pub id:         Uuid,
    pub user_id:    Uuid,
    pub token_hash: String,   // SHA-256 hex of the raw token
    pub kind:       String,   // "email_verify" | "password_reset"
    pub expires_at: DateTime<Utc>,
    pub used_at:    Option<DateTime<Utc>>,
    pub created_at: DateTime<Utc>,
}

// ── API tokens (CLI) ──────────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize, sqlx::FromRow)]
pub struct ApiToken {
    pub id:          Uuid,
    pub user_id:     Uuid,
    pub name:        String,
    pub token_hash:  String,   // SHA-256(raw_token)
    pub prefix:      String,   // first 8 chars of raw token (display only)
    pub last_used_at: Option<DateTime<Utc>>,
    pub expires_at:  Option<DateTime<Utc>>,
    pub created_at:  DateTime<Utc>,
}

// ── Packages ──────────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize, sqlx::FromRow)]
pub struct Package {
    pub id:          Uuid,
    pub name:        String,
    pub description: Option<String>,
    pub repository:  Option<String>,
    pub homepage:    Option<String>,
    pub license:     Option<String>,
    pub readme:      Option<String>,
    // JSON arrays stored as TEXT in Postgres, deserialized by queries
    pub keywords:    sqlx::types::Json<Vec<String>>,
    pub categories:  sqlx::types::Json<Vec<String>>,
    pub downloads:   i64,
    pub created_at:  DateTime<Utc>,
    pub updated_at:  DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize, sqlx::FromRow)]
pub struct PackageSummary {
    pub id:          Uuid,
    pub name:        String,
    pub description: Option<String>,
    pub license:     Option<String>,
    pub downloads:   i64,
    pub latest_version: Option<String>,
    pub updated_at:  DateTime<Utc>,
}

// ── Versions ──────────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize, sqlx::FromRow)]
pub struct PackageVersion {
    pub id:           Uuid,
    pub package_id:   Uuid,
    pub version:      String,
    pub checksum:     String,   // SHA-256 of the uploaded .tar.gz (hex)
    pub size_bytes:   i64,
    pub s3_key:       String,
    pub readme:       Option<String>,
    pub manifest:     sqlx::types::Json<serde_json::Value>,
    pub yanked:       bool,
    pub yank_reason:  Option<String>,
    pub downloads:    i64,
    pub published_by: Uuid,    // user id
    pub created_at:   DateTime<Utc>,
}

// ── Ownership ─────────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize, sqlx::FromRow)]
pub struct PackageOwner {
    pub package_id: Uuid,
    pub user_id:    Uuid,
    pub role:       String,   // "owner" | "collaborator"
    pub invited_by: Option<Uuid>,
    pub created_at: DateTime<Utc>,
}

// ── Ownership invites ─────────────────────────────────────────────────────────

#[derive(Debug, Clone, sqlx::FromRow)]
pub struct OwnershipInvite {
    pub id:          Uuid,
    pub package_id:  Uuid,
    pub invitee_id:  Uuid,
    pub inviter_id:  Uuid,
    pub token_hash:  String,
    pub role:        String,
    pub accepted_at: Option<DateTime<Utc>>,
    pub expires_at:  DateTime<Utc>,
    pub created_at:  DateTime<Utc>,
}

// ── Download log ──────────────────────────────────────────────────────────────

#[derive(Debug, Clone, sqlx::FromRow)]
pub struct DownloadLog {
    pub id:         Uuid,
    pub version_id: Uuid,
    pub ip_hash:    String,   // SHA-256(ip) — never stored in plain text
    pub user_agent: Option<String>,
    pub created_at: DateTime<Utc>,
}

// ── Audit log ────────────────────────────────────────────────────────────────

#[derive(Debug, Clone, sqlx::FromRow)]
pub struct AuditEntry {
    pub id:         Uuid,
    pub actor_id:   Option<Uuid>,
    pub action:     String,
    pub target_id:  Option<Uuid>,
    pub target_type: Option<String>,
    pub metadata:   sqlx::types::Json<serde_json::Value>,
    pub created_at: DateTime<Utc>,
}
