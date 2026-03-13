//! Minimal SemVer dependency resolver.
//!
//! Given a direct dependency set, queries the DB index to find the highest
//! compatible version for each dependency. Does not do full SAT solving —
//! for a first version, a simple "take the latest compatible" greedy approach
//! is sufficient and covers the common case.

use crate::IndexEntry;
use dal_common::error::DalError;
use semver::{Version, VersionReq};
use std::collections::HashMap;
use tracing::debug;

/// Resolve a flat dependency map `{ name → req }` against a set of available
/// index entries (keyed by package name).
///
/// Returns a map of `{ name → resolved_version_string }`.
pub fn resolve(
    deps: &HashMap<String, String>,
    available: &HashMap<String, Vec<IndexEntry>>,
) -> Result<HashMap<String, String>, DalError> {
    let mut resolved = HashMap::new();

    for (name, req_str) in deps {
        let req = VersionReq::parse(req_str).map_err(|_| {
            DalError::Validation(format!(
                "invalid version requirement `{req_str}` for dependency `{name}`"
            ))
        })?;

        let versions = available
            .get(name)
            .ok_or_else(|| DalError::PackageNotFound(name.clone()))?;

        // Collect non-yanked versions that satisfy the requirement
        let mut candidates: Vec<Version> = versions
            .iter()
            .filter(|e| !e.yanked)
            .filter_map(|e| Version::parse(&e.vers).ok())
            .filter(|v| req.matches(v))
            .collect();

        // Sort descending, pick highest
        candidates.sort_unstable_by(|a, b| b.cmp(a));

        let chosen = candidates.into_iter().next().ok_or_else(|| {
            DalError::Validation(format!("no version of `{name}` satisfies `{req_str}`"))
        })?;

        debug!(package = name, version = %chosen, "resolved dependency");
        resolved.insert(name.clone(), chosen.to_string());
    }

    Ok(resolved)
}
