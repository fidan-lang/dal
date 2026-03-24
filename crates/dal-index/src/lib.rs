//! Sparse package index — serves one NDJSON file per package name.
//!
//! Layout mirrors the Cargo sparse index protocol for familiarity:
//!   GET /index/{name}    → NDJSON, one JSON object per published version
//!
//! Each line is an `IndexEntry` serialised to JSON.

use serde::{Deserialize, Serialize};

mod resolver;
pub use resolver::resolve;

/// One line in the NDJSON index file — describes a single published version.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IndexEntry {
    /// Package name.
    pub name: String,
    /// SemVer version string.
    pub vers: String,
    /// Dependency list.
    pub deps: Vec<IndexDep>,
    /// SHA-256 hex checksum of the `.tar.gz`.
    pub cksum: String,
    /// Whether the version has been yanked.
    pub yanked: bool,
    /// SPDX license string (optional, for tooling).
    pub license: Option<String>,
}

/// A dependency as recorded in the index.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IndexDep {
    pub name: String,
    pub req: String, // SemVer requirement, e.g. "^1.0"
    pub optional: bool,
    pub kind: DepKind,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum DepKind {
    Normal,
    Dev,
}

/// Build the NDJSON body for a package's index file from all its versions.
pub fn build_index_ndjson(entries: &[IndexEntry]) -> String {
    entries
        .iter()
        .filter_map(|e| serde_json::to_string(e).ok())
        .collect::<Vec<_>>()
        .join("\n")
}

/// Build an `IndexEntry` from a `PackageVersion` row and its parsed manifest.
pub fn entry_from_version(
    name: &str,
    version: &str,
    checksum: &str,
    yanked: bool,
    license: Option<&str>,
    manifest: &dal_manifest::Manifest,
) -> IndexEntry {
    let mut deps: Vec<IndexDep> = Vec::new();

    for (dep_name, spec) in &manifest.dependencies {
        deps.push(IndexDep {
            name: dep_name.clone(),
            req: spec.version_req().to_string(),
            optional: spec.is_optional(),
            kind: DepKind::Normal,
        });
    }

    for (dep_name, spec) in &manifest.optional_dependencies {
        deps.push(IndexDep {
            name: dep_name.clone(),
            req: spec.version_req().to_string(),
            optional: true,
            kind: DepKind::Normal,
        });
    }

    for (dep_name, spec) in &manifest.dev_dependencies {
        deps.push(IndexDep {
            name: dep_name.clone(),
            req: spec.version_req().to_string(),
            optional: false,
            kind: DepKind::Dev,
        });
    }

    IndexEntry {
        name: name.to_string(),
        vers: version.to_string(),
        deps,
        cksum: checksum.to_string(),
        yanked,
        license: license.map(str::to_string),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use dal_manifest::Manifest;

    #[test]
    fn index_entry_includes_optional_dependencies() {
        let manifest: Manifest = r#"
[package]
name = "torch"
version = "1.0.0"

[dependencies]
core = "^1"

[optional-dependencies]
python-runtime = "^3"
"#
        .parse()
        .expect("valid manifest");

        let entry = entry_from_version("torch", "1.0.0", "abc", false, None, &manifest);
        assert_eq!(entry.deps.len(), 2);
        assert!(
            entry
                .deps
                .iter()
                .any(|dep| dep.name == "python-runtime" && dep.optional)
        );
        assert!(
            entry
                .deps
                .iter()
                .any(|dep| dep.name == "core" && !dep.optional)
        );
    }
}
