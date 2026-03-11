use dal_common::error::DalError;
use sha2::{Digest, Sha256};
use std::io::Read;

/// Information extracted from a validated `.tar.gz` package archive.
pub struct ArchiveInfo {
    /// SHA-256 hex digest of the raw compressed bytes.
    pub checksum:       String,
    /// Compressed size in bytes.
    pub size_bytes:     i64,
    /// Raw bytes of `dal.toml` if found in the archive root.
    pub manifest_bytes: Option<Vec<u8>>,
    /// Raw bytes of the README file (README.md / readme.md) if present.
    pub readme_bytes:   Option<Vec<u8>>,
}

/// Validate a `.tar.gz` archive and extract key metadata.
///
/// Security checks performed:
/// - Absolute paths rejected
/// - Path traversal (`..`) rejected
/// - Archive ratio (uncompressed / compressed) must be < 100 (anti-bomb)
/// - Total compressed size checked against `max_bytes`
pub fn validate_archive(bytes: &[u8], max_bytes: u64) -> Result<ArchiveInfo, DalError> {
    if bytes.len() as u64 > max_bytes {
        return Err(DalError::ArchiveTooLarge {
            max_mb: max_bytes / (1024 * 1024),
        });
    }

    // Compute checksum of compressed bytes
    let mut hasher = Sha256::new();
    hasher.update(bytes);
    let checksum = hex::encode(hasher.finalize());

    // Decompress and iterate entries
    let gz = flate2::read::GzDecoder::new(bytes);
    let mut archive = tar::Archive::new(gz);

    let compressed_size = bytes.len() as u64;
    let mut uncompressed_size: u64 = 0;
    let mut manifest_bytes: Option<Vec<u8>> = None;
    let mut readme_bytes: Option<Vec<u8>> = None;

    let entries = archive
        .entries()
        .map_err(|e| DalError::ArchiveInvalid(format!("cannot read archive entries: {e}")))?;

    for entry in entries {
        let mut entry = entry
            .map_err(|e| DalError::ArchiveInvalid(format!("bad archive entry: {e}")))?;

        let path = entry
            .path()
            .map_err(|e| DalError::ArchiveInvalid(format!("bad path in archive: {e}")))?;

        // Security: reject absolute paths
        if path.is_absolute() {
            return Err(DalError::ArchiveInvalid(
                "archive contains an absolute path".into(),
            ));
        }

        // Security: reject any `..` component
        for component in path.components() {
            use std::path::Component;
            if matches!(component, Component::ParentDir) {
                return Err(DalError::ArchiveInvalid(
                    "archive contains path traversal (..)".into(),
                ));
            }
        }

        let size = entry.header().size()
            .map_err(|e| DalError::ArchiveInvalid(format!("bad entry size: {e}")))?;
        uncompressed_size = uncompressed_size.saturating_add(size);

        // Archive bomb: ratio check
        if compressed_size > 0 && uncompressed_size / compressed_size > 100 {
            return Err(DalError::ArchiveInvalid(
                "archive expansion ratio exceeds 100:1 (possible archive bomb)".into(),
            ));
        }

        // Collect dal.toml and README from archive root (first path component)
        let path_str = path.to_string_lossy();
        // Strip optional leading "packagename-version/" or "./" prefix
        let normalized: &str = {
            let s = path_str.trim_start_matches("./");
            // If there's a single directory prefix, strip it
            if let Some(rest) = s.split_once('/') {
                rest.1
            } else {
                s
            }
        };

        if normalized == "dal.toml" && manifest_bytes.is_none() {
            let mut buf = Vec::new();
            entry.read_to_end(&mut buf)
                .map_err(|e| DalError::ArchiveInvalid(format!("cannot read dal.toml: {e}")))?;
            manifest_bytes = Some(buf);
        } else if matches!(normalized.to_lowercase().as_str(), "readme.md" | "readme.txt" | "readme")
            && readme_bytes.is_none()
        {
            let mut buf = Vec::new();
            entry.read_to_end(&mut buf)
                .map_err(|e| DalError::ArchiveInvalid(format!("cannot read README: {e}")))?;
            // Limit README to 512 KiB
            if buf.len() > 512 * 1024 {
                buf.truncate(512 * 1024);
            }
            readme_bytes = Some(buf);
        }
    }

    Ok(ArchiveInfo {
        checksum,
        size_bytes: bytes.len() as i64,
        manifest_bytes,
        readme_bytes,
    })
}
