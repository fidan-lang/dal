use sha2::{Digest, Sha256};

/// A freshly generated CLI API token.
pub struct ApiTokenRaw {
    /// The full raw token to hand to the user once (never stored).
    pub raw: String,
    /// SHA-256 hex digest — stored in the database.
    pub hash: String,
    /// First 8 chars of the raw token — stored for display (safe to show).
    pub prefix: String,
}

/// Generate a new API token in the form `dal_<64 hex chars>`.
pub fn generate_api_token() -> ApiTokenRaw {
    let mut bytes = [0u8; 32];
    rand::fill(&mut bytes);
    let hex_part = hex::encode(bytes);
    let raw = format!("dal_{hex_part}");
    let hash = hash_token(&raw);
    let prefix = raw[..8.min(raw.len())].to_string();
    ApiTokenRaw { raw, hash, prefix }
}

/// SHA-256 hash of a raw token (stored in DB, never the plain token).
pub fn hash_token(raw: &str) -> String {
    let mut hasher = Sha256::new();
    hasher.update(raw.as_bytes());
    hex::encode(hasher.finalize())
}

/// Hash an IP address for privacy-preserving download logging.
pub fn hash_ip(ip: &str) -> String {
    let mut hasher = Sha256::new();
    hasher.update(ip.as_bytes());
    hex::encode(hasher.finalize())
}

/// Validate the format of a raw API token (`dal_` + 64 hex chars).
pub fn validate_api_token_format(raw: &str) -> bool {
    raw.starts_with("dal_")
        && raw.len() == 68  // "dal_" (4) + 64 hex chars
        && raw[4..].chars().all(|c| c.is_ascii_hexdigit())
}
