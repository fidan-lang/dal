use dal_common::error::DalError;
use sha2::{Digest, Sha256};
use std::collections::BTreeSet;
use std::io::Read;
use std::path::{Component, Path};

/// Information extracted from a validated `.tar.gz` package archive.
#[derive(Debug)]
pub struct ArchiveInfo {
    /// SHA-256 hex digest of the raw compressed bytes.
    pub checksum: String,
    /// Compressed size in bytes.
    pub size_bytes: i64,
    /// Canonical top-level package directory.
    pub root_dir: String,
    /// Raw bytes of `dal.toml` if found in the archive root.
    pub manifest_bytes: Option<Vec<u8>>,
    /// Relative file paths contained within the package root.
    pub files: BTreeSet<String>,
    /// Raw bytes of the README file (README.md / readme.md) if present.
    pub readme_bytes: Option<Vec<u8>>,
    /// Relative path of the extracted default README file, if present.
    pub readme_path: Option<String>,
}

/// Validate a `.tar.gz` archive and extract key metadata.
///
/// Security checks performed:
/// - Absolute paths rejected
/// - Path traversal (`..`) rejected
/// - Archive must contain exactly one top-level package directory
/// - Only regular files and directories are allowed
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
    let mut top_level_entries = BTreeSet::new();
    let mut manifest_bytes: Option<Vec<u8>> = None;
    let mut files = BTreeSet::new();
    let mut readme_bytes: Option<Vec<u8>> = None;
    let mut readme_path: Option<String> = None;
    let mut has_init_file = false;

    let entries = archive
        .entries()
        .map_err(|e| DalError::ArchiveInvalid(format!("cannot read archive entries: {e}")))?;

    for entry in entries {
        let mut entry =
            entry.map_err(|e| DalError::ArchiveInvalid(format!("bad archive entry: {e}")))?;
        let entry_type = entry.header().entry_type();

        let path = entry
            .path()
            .map_err(|e| DalError::ArchiveInvalid(format!("bad path in archive: {e}")))?;

        if entry_type.is_symlink()
            || entry_type.is_hard_link()
            || entry_type.is_block_special()
            || entry_type.is_character_special()
            || entry_type.is_fifo()
        {
            return Err(DalError::ArchiveInvalid(
                "archive contains unsupported link or special file entries".into(),
            ));
        }

        let components = normalize_entry_components(&path)?;
        if components.is_empty() {
            continue;
        }

        let size = entry
            .header()
            .size()
            .map_err(|e| DalError::ArchiveInvalid(format!("bad entry size: {e}")))?;
        uncompressed_size = uncompressed_size.saturating_add(size);

        // Archive bomb: ratio check
        if compressed_size > 0 && uncompressed_size / compressed_size > 100 {
            return Err(DalError::ArchiveInvalid(
                "archive expansion ratio exceeds 100:1 (possible archive bomb)".into(),
            ));
        }

        let root = components[0].clone();
        top_level_entries.insert(root);

        if components.len() == 1 {
            if entry_type.is_file() {
                return Err(DalError::ArchiveInvalid(
                    "archive files must be contained in a single top-level package directory"
                        .into(),
                ));
            }
            continue;
        }

        validate_top_level_entry(&components)?;

        let normalized = components[1..].join("/");

        if entry_type.is_file() {
            files.insert(normalized.clone());
        }

        if normalized == "dal.toml" && entry_type.is_file() {
            if manifest_bytes.is_some() {
                return Err(DalError::ArchiveInvalid(
                    "archive contains multiple dal.toml files".into(),
                ));
            }

            let mut buf = Vec::new();
            entry
                .read_to_end(&mut buf)
                .map_err(|e| DalError::ArchiveInvalid(format!("cannot read dal.toml: {e}")))?;
            manifest_bytes = Some(buf);
        } else if matches!(
            normalized.to_lowercase().as_str(),
            "readme.md" | "readme.txt" | "readme"
        ) && entry_type.is_file()
            && readme_bytes.is_none()
        {
            let mut buf = Vec::new();
            entry
                .read_to_end(&mut buf)
                .map_err(|e| DalError::ArchiveInvalid(format!("cannot read README: {e}")))?;
            // Limit README to 512 KiB
            if buf.len() > 512 * 1024 {
                buf.truncate(512 * 1024);
            }
            readme_bytes = Some(buf);
            readme_path = Some(normalized);
        } else if entry_type.is_file() && normalized == "src/init.fdn" {
            has_init_file = true;
        }
    }

    let root_dir = match top_level_entries.len() {
        0 => {
            return Err(DalError::ArchiveInvalid(
                "archive is empty or does not contain package files".into(),
            ));
        }
        1 => top_level_entries.into_iter().next().unwrap_or_default(),
        _ => {
            return Err(DalError::ArchiveInvalid(
                "archive must contain exactly one top-level package directory".into(),
            ));
        }
    };

    if !has_init_file {
        return Err(DalError::ArchiveInvalid(
            "archive must contain src/init.fdn".into(),
        ));
    }

    Ok(ArchiveInfo {
        checksum,
        size_bytes: bytes.len() as i64,
        root_dir,
        manifest_bytes,
        files,
        readme_bytes,
        readme_path,
    })
}

pub fn extract_file(
    bytes: &[u8],
    root_dir: &str,
    rel_path: &str,
) -> Result<Option<Vec<u8>>, DalError> {
    let target_rel = normalize_relative_path(rel_path)?;
    let gz = flate2::read::GzDecoder::new(bytes);
    let mut archive = tar::Archive::new(gz);

    let entries = archive
        .entries()
        .map_err(|e| DalError::ArchiveInvalid(format!("cannot read archive entries: {e}")))?;

    for entry in entries {
        let mut entry =
            entry.map_err(|e| DalError::ArchiveInvalid(format!("bad archive entry: {e}")))?;
        let entry_type = entry.header().entry_type();
        if !entry_type.is_file() {
            continue;
        }

        let path = entry
            .path()
            .map_err(|e| DalError::ArchiveInvalid(format!("bad path in archive: {e}")))?;
        let components = normalize_entry_components(&path)?;
        if components.len() < 2 || components[0] != root_dir {
            continue;
        }

        let normalized = components[1..].join("/");
        if normalized == target_rel {
            let mut buf = Vec::new();
            entry
                .read_to_end(&mut buf)
                .map_err(|e| DalError::ArchiveInvalid(format!("cannot read {rel_path}: {e}")))?;
            return Ok(Some(buf));
        }
    }

    Ok(None)
}

fn normalize_entry_components(path: &Path) -> Result<Vec<String>, DalError> {
    let mut components = Vec::new();
    for component in path.components() {
        match component {
            Component::CurDir => {}
            Component::Normal(part) => components.push(part.to_string_lossy().into_owned()),
            Component::ParentDir => {
                return Err(DalError::ArchiveInvalid(
                    "archive contains path traversal (..)".into(),
                ));
            }
            Component::RootDir | Component::Prefix(_) => {
                return Err(DalError::ArchiveInvalid(
                    "archive contains an absolute path".into(),
                ));
            }
        }
    }
    Ok(components)
}

fn normalize_relative_path(path: &str) -> Result<String, DalError> {
    let components = normalize_entry_components(Path::new(path))?;
    if components.is_empty() {
        return Err(DalError::ArchiveInvalid(
            "archive path must not be empty".into(),
        ));
    }
    Ok(components.join("/"))
}

fn validate_top_level_entry(components: &[String]) -> Result<(), DalError> {
    if components.len() < 2 {
        return Ok(());
    }

    let top_level = &components[1];
    if components.len() == 2 {
        if is_allowed_top_level_file(top_level) || is_allowed_top_level_dir(top_level) {
            return Ok(());
        }
    } else if is_allowed_top_level_dir(top_level) {
        return Ok(());
    }

    Err(DalError::ArchiveInvalid(format!(
        "top-level package entry `{top_level}` is not allowed"
    )))
}

fn is_allowed_top_level_dir(name: &str) -> bool {
    matches!(name, "src" | "examples" | "tests" | "docs" | "assets")
}

fn is_allowed_top_level_file(name: &str) -> bool {
    let lower = name.to_ascii_lowercase();
    lower == "dal.toml"
        || lower == "readme"
        || lower == "readme.md"
        || lower == "readme.txt"
        || lower == "changelog.md"
        || lower == "license"
        || lower.starts_with("license.")
}

#[cfg(test)]
mod tests {
    use super::*;
    use flate2::{Compression, write::GzEncoder};
    use std::io::Write;
    use tar::{Builder, EntryType, Header};

    fn archive_bytes(entries: Vec<(&str, EntryType, &[u8])>) -> Vec<u8> {
        let mut tar_bytes = Vec::new();
        {
            let mut builder = Builder::new(&mut tar_bytes);
            for (path, entry_type, contents) in entries {
                let mut header = Header::new_gnu();
                header.set_entry_type(entry_type);
                header.set_mode(0o644);
                header.set_size(contents.len() as u64);
                header.set_cksum();
                builder
                    .append_data(&mut header, path, contents)
                    .expect("append tar entry");
            }
            builder.finish().expect("finish tar");
        }

        let mut gz = GzEncoder::new(Vec::new(), Compression::default());
        gz.write_all(&tar_bytes).expect("write gzip");
        gz.finish().expect("finish gzip")
    }

    #[test]
    fn accepts_canonical_package_layout() {
        let bytes = archive_bytes(vec![
            (
                "demo-0.1.0/dal.toml",
                EntryType::Regular,
                b"[package]\nname = \"demo\"\nversion = \"0.1.0\"\n",
            ),
            (
                "demo-0.1.0/src/init.fdn",
                EntryType::Regular,
                b"action main {}",
            ),
            ("demo-0.1.0/README.md", EntryType::Regular, b"# demo"),
            ("demo-0.1.0/assets/logo.png", EntryType::Regular, b"png"),
        ]);

        let info = validate_archive(&bytes, 1024 * 1024).expect("valid archive");
        assert_eq!(info.root_dir, "demo-0.1.0");
        assert!(info.files.contains("dal.toml"));
        assert!(info.files.contains("src/init.fdn"));
        assert!(info.files.contains("assets/logo.png"));
        assert_eq!(info.readme_path.as_deref(), Some("README.md"));
    }

    #[test]
    fn rejects_flat_root_files() {
        let bytes = archive_bytes(vec![
            (
                "dal.toml",
                EntryType::Regular,
                b"[package]\nname = \"demo\"\nversion = \"0.1.0\"\n",
            ),
            ("src/init.fdn", EntryType::Regular, b"action main {}"),
        ]);

        let err = validate_archive(&bytes, 1024 * 1024).expect_err("flat root must fail");
        assert!(
            err.to_string()
                .contains("single top-level package directory")
        );
    }

    #[test]
    fn rejects_multiple_top_level_directories() {
        let bytes = archive_bytes(vec![
            (
                "demo-0.1.0/dal.toml",
                EntryType::Regular,
                b"[package]\nname = \"demo\"\nversion = \"0.1.0\"\n",
            ),
            (
                "demo-0.1.0/src/init.fdn",
                EntryType::Regular,
                b"action main {}",
            ),
            (
                "other-0.1.0/src/extra.fdn",
                EntryType::Regular,
                b"action extra {}",
            ),
        ]);

        let err = validate_archive(&bytes, 1024 * 1024).expect_err("multiple roots must fail");
        assert!(
            err.to_string()
                .contains("exactly one top-level package directory")
        );
    }

    #[test]
    fn rejects_symlink_entries() {
        let bytes = archive_bytes(vec![
            (
                "demo-0.1.0/dal.toml",
                EntryType::Regular,
                b"[package]\nname = \"demo\"\nversion = \"0.1.0\"\n",
            ),
            (
                "demo-0.1.0/src/init.fdn",
                EntryType::Regular,
                b"action main {}",
            ),
            ("demo-0.1.0/src/link.fdn", EntryType::Symlink, b""),
        ]);

        let err = validate_archive(&bytes, 1024 * 1024).expect_err("symlink must fail");
        assert!(err.to_string().contains("unsupported link or special file"));
    }

    #[test]
    fn rejects_unknown_top_level_file() {
        let bytes = archive_bytes(vec![
            (
                "demo-0.1.0/dal.toml",
                EntryType::Regular,
                b"[package]\nname = \"demo\"\nversion = \"0.1.0\"\n",
            ),
            (
                "demo-0.1.0/src/init.fdn",
                EntryType::Regular,
                b"action main {}",
            ),
            ("demo-0.1.0/package.json", EntryType::Regular, b"{}"),
        ]);

        let err =
            validate_archive(&bytes, 1024 * 1024).expect_err("unknown top-level file must fail");
        assert!(
            err.to_string()
                .contains("top-level package entry `package.json` is not allowed")
        );
    }

    #[test]
    fn accepts_assets_directory() {
        let bytes = archive_bytes(vec![
            (
                "demo-0.1.0/dal.toml",
                EntryType::Regular,
                b"[package]\nname = \"demo\"\nversion = \"0.1.0\"\n",
            ),
            (
                "demo-0.1.0/src/init.fdn",
                EntryType::Regular,
                b"action main {}",
            ),
            ("demo-0.1.0/assets/logo.png", EntryType::Regular, b"png"),
        ]);

        let info = validate_archive(&bytes, 1024 * 1024).expect("assets directory is allowed");
        assert!(info.files.contains("assets/logo.png"));
    }

    #[test]
    fn rejects_missing_src_init_file() {
        let bytes = archive_bytes(vec![
            (
                "demo-0.1.0/dal.toml",
                EntryType::Regular,
                b"[package]\nname = \"demo\"\nversion = \"0.1.0\"\n",
            ),
            (
                "demo-0.1.0/src/main.fdn",
                EntryType::Regular,
                b"action main {}",
            ),
        ]);

        let err =
            validate_archive(&bytes, 1024 * 1024).expect_err("missing src/init.fdn must fail");
        assert!(
            err.to_string()
                .contains("archive must contain src/init.fdn")
        );
    }

    #[test]
    fn extracts_declared_file_under_root() {
        let bytes = archive_bytes(vec![
            (
                "demo-0.1.0/dal.toml",
                EntryType::Regular,
                b"[package]\nname = \"demo\"\nversion = \"0.1.0\"\n",
            ),
            (
                "demo-0.1.0/src/init.fdn",
                EntryType::Regular,
                b"action main {}",
            ),
            ("demo-0.1.0/docs/README.txt", EntryType::Regular, b"hello"),
        ]);

        let file = extract_file(&bytes, "demo-0.1.0", "docs/README.txt")
            .expect("extract succeeds")
            .expect("file exists");
        assert_eq!(file, b"hello");
    }
}
