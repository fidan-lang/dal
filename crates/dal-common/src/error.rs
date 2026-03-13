use axum::{
    Json,
    http::StatusCode,
    response::{IntoResponse, Response},
};
use serde_json::json;
use thiserror::Error;

pub type Result<T, E = DalError> = std::result::Result<T, E>;

#[derive(Debug, Error)]
pub enum DalError {
    // ── Auth ─────────────────────────────────────────────────────────────────
    #[error("invalid or expired token")]
    Unauthorized,
    #[error("you do not have permission to perform this action")]
    Forbidden,
    #[error("invalid API token format")]
    InvalidApiToken,

    // ── Resources ────────────────────────────────────────────────────────────
    #[error("package `{0}` not found")]
    PackageNotFound(String),
    #[error("version `{0}` of package `{1}` not found")]
    VersionNotFound(String, String),
    #[error("user `{0}` not found")]
    UserNotFound(String),

    // ── Conflicts ────────────────────────────────────────────────────────────
    #[error("package name `{0}` is already taken")]
    PackageNameTaken(String),
    #[error("version `{0}` already exists for package `{1}`")]
    VersionAlreadyExists(String, String),
    #[error("username `{0}` is already taken")]
    UsernameTaken(String),
    #[error("email is already registered")]
    EmailTaken,

    // ── Validation ───────────────────────────────────────────────────────────
    #[error("validation error: {0}")]
    Validation(String),
    #[error("archive is too large (max {max_mb} MiB)")]
    ArchiveTooLarge { max_mb: u64 },
    #[error("archive is invalid: {0}")]
    ArchiveInvalid(String),
    #[error("manifest (dal.toml) is missing or invalid: {0}")]
    ManifestInvalid(String),

    // ── External services ────────────────────────────────────────────────────
    #[error("database error: {0}")]
    Database(#[from] sqlx::Error),
    #[error("storage error: {0}")]
    Storage(String),
    #[error("email error: {0}")]
    Email(String),
    #[error("aws cognito error: {0}")]
    Cognito(String),
    #[error("sqs error: {0}")]
    Sqs(String),

    // ── Rate limiting ─────────────────────────────────────────────────────────
    #[error("rate limit exceeded — try again later")]
    RateLimited,

    // ── Catch-all ─────────────────────────────────────────────────────────────
    #[error("internal server error")]
    Internal(#[from] anyhow::Error),
}

impl DalError {
    pub fn status(&self) -> StatusCode {
        match self {
            Self::Unauthorized | Self::InvalidApiToken => StatusCode::UNAUTHORIZED,
            Self::Forbidden => StatusCode::FORBIDDEN,
            Self::PackageNotFound(_) | Self::VersionNotFound(_, _) | Self::UserNotFound(_) => {
                StatusCode::NOT_FOUND
            }
            Self::PackageNameTaken(_)
            | Self::VersionAlreadyExists(_, _)
            | Self::UsernameTaken(_)
            | Self::EmailTaken => StatusCode::CONFLICT,
            Self::Validation(_)
            | Self::ManifestInvalid(_)
            | Self::ArchiveTooLarge { .. }
            | Self::ArchiveInvalid(_) => StatusCode::UNPROCESSABLE_ENTITY,
            Self::RateLimited => StatusCode::TOO_MANY_REQUESTS,
            _ => StatusCode::INTERNAL_SERVER_ERROR,
        }
    }

    pub fn code(&self) -> &'static str {
        match self {
            Self::Unauthorized => "UNAUTHORIZED",
            Self::Forbidden => "FORBIDDEN",
            Self::InvalidApiToken => "INVALID_API_TOKEN",
            Self::PackageNotFound(_) => "PACKAGE_NOT_FOUND",
            Self::VersionNotFound(_, _) => "VERSION_NOT_FOUND",
            Self::UserNotFound(_) => "USER_NOT_FOUND",
            Self::PackageNameTaken(_) => "PACKAGE_NAME_TAKEN",
            Self::VersionAlreadyExists(_, _) => "VERSION_ALREADY_EXISTS",
            Self::UsernameTaken(_) => "USERNAME_TAKEN",
            Self::EmailTaken => "EMAIL_TAKEN",
            Self::Validation(_) => "VALIDATION_ERROR",
            Self::ArchiveTooLarge { .. } => "ARCHIVE_TOO_LARGE",
            Self::ArchiveInvalid(_) => "ARCHIVE_INVALID",
            Self::ManifestInvalid(_) => "MANIFEST_INVALID",
            Self::Database(_) => "DATABASE_ERROR",
            Self::Storage(_) => "STORAGE_ERROR",
            Self::Email(_) => "EMAIL_ERROR",
            Self::Cognito(_) => "COGNITO_ERROR",
            Self::Sqs(_) => "SQS_ERROR",
            Self::RateLimited => "RATE_LIMITED",
            Self::Internal(_) => "INTERNAL_ERROR",
        }
    }
}

impl IntoResponse for DalError {
    fn into_response(self) -> Response {
        let status = self.status();
        let code = self.code();
        let message = self.to_string();

        if status == StatusCode::INTERNAL_SERVER_ERROR {
            tracing::error!(code, %message, "internal error");
        } else {
            tracing::debug!(code, %message, "request error");
        }

        (
            status,
            Json(json!({
                "error": {
                    "code":    code,
                    "message": message
                }
            })),
        )
            .into_response()
    }
}
