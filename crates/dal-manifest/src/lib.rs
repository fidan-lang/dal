mod validate;

use semver::Version;
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};
use std::str::FromStr;
use url::Url;

pub use validate::validate_package_name;

const LOCK_SCHEMA_VERSION: u32 = 1;

/// Parsed and validated `dal.toml` manifest.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Manifest {
    pub package: PackageMeta,
    #[serde(default)]
    pub dependencies: HashMap<String, DependencySpec>,
    #[serde(default)]
    pub dev_dependencies: HashMap<String, DependencySpec>,
    #[serde(default)]
    pub cli: Option<CliMeta>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PackageMeta {
    /// Unique package name on the registry (e.g. `my-pkg`).
    pub name: String,
    /// SemVer version string.
    pub version: String,
    /// One-line description of the package.
    pub description: Option<String>,
    /// SPDX license identifier (e.g. `MIT`).
    pub license: Option<String>,
    /// Repository URL.
    pub repository: Option<String>,
    /// Package homepage URL.
    pub homepage: Option<String>,
    /// Documentation URL.
    pub docs: Option<String>,
    /// List of keywords (max 5, each max 20 chars).
    #[serde(default)]
    pub keywords: Vec<String>,
    /// List of categories (max 5).
    #[serde(default)]
    pub categories: Vec<String>,
    /// README file path relative to the package root (defaults to `README.md`).
    pub readme: Option<String>,
    /// Files to include (glob patterns). Defaults to everything.
    #[serde(default)]
    pub include: Vec<String>,
    /// Files to exclude (glob patterns).
    #[serde(default)]
    pub exclude: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CliMeta {
    pub entry: String,
    pub name: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Lockfile {
    pub schema_version: u32,
    pub packages: Vec<LockedPackage>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LockedPackage {
    pub name: String,
    pub module: String,
    pub version: String,
    #[serde(default)]
    pub dependencies: HashMap<String, String>,
}

/// A dependency specification.
///
/// In `dal.toml` this can be written in two ways:
///     my-pkg = "^1.0"
///     my-pkg = { version = "^1.0", optional = true }
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(untagged)]
pub enum DependencySpec {
    /// Short form: just a version requirement string.
    Simple(String),
    /// Detailed form with optional fields.
    Detailed {
        version: String,
        #[serde(default)]
        optional: bool,
        /// Pin to an exact git repository instead of the registry.
        git: Option<String>,
        rev: Option<String>,
    },
}

impl DependencySpec {
    /// Returns the version requirement string.
    pub fn version_req(&self) -> &str {
        match self {
            Self::Simple(v) => v,
            Self::Detailed { version, .. } => version,
        }
    }
}

impl Manifest {
    /// Parse a `dal.toml` byte slice.
    pub fn from_toml(bytes: &[u8]) -> Result<Self, ManifestError> {
        let text = std::str::from_utf8(bytes).map_err(|_| ManifestError::NotUtf8)?;
        text.parse()
    }

    /// Returns the parsed SemVer version.
    pub fn version(&self) -> Result<Version, ManifestError> {
        Version::parse(&self.package.version)
            .map_err(|_| ManifestError::InvalidVersion(self.package.version.clone()))
    }

    fn validate(&self) -> Result<(), ManifestError> {
        validate_package_name(&self.package.name)?;

        // Validate version is parseable SemVer
        self.version()?;

        // Validate keywords
        if self.package.keywords.len() > 5 {
            return Err(ManifestError::TooManyKeywords);
        }
        for kw in &self.package.keywords {
            if kw.len() > 20 {
                return Err(ManifestError::KeywordTooLong(kw.clone()));
            }
        }

        // Validate categories
        if self.package.categories.len() > 5 {
            return Err(ManifestError::TooManyCategories);
        }

        // Validate URLs
        for (field, url) in [
            ("repository", self.package.repository.as_deref()),
            ("homepage", self.package.homepage.as_deref()),
            ("docs", self.package.docs.as_deref()),
        ] {
            if let Some(u) = url {
                Url::parse(u).map_err(|_| ManifestError::InvalidUrl {
                    field: field.into(),
                    value: u.into(),
                })?;
            }
        }

        // Validate dependency version requirements
        for (name, dep) in self.dependencies.iter().chain(self.dev_dependencies.iter()) {
            semver::VersionReq::parse(dep.version_req()).map_err(|_| {
                ManifestError::InvalidVersionReq {
                    dep: name.clone(),
                    req: dep.version_req().into(),
                }
            })?;
        }

        if let Some(cli) = &self.cli {
            if cli.entry.trim().is_empty() {
                return Err(ManifestError::InvalidCliEntry(
                    "`[cli].entry` must not be empty".into(),
                ));
            }
            let entry_path = std::path::Path::new(&cli.entry);
            if entry_path.is_absolute() {
                return Err(ManifestError::InvalidCliEntry(
                    "`[cli].entry` must be a relative path".into(),
                ));
            }
            if !is_safe_relative_path(entry_path) {
                return Err(ManifestError::InvalidCliEntry(
                    "`[cli].entry` contains an unsafe path".into(),
                ));
            }
            if entry_path.extension().and_then(|ext| ext.to_str()) != Some("fdn") {
                return Err(ManifestError::InvalidCliEntry(
                    "`[cli].entry` must point to a `.fdn` file".into(),
                ));
            }
            if let Some(name) = &cli.name {
                validate_cli_binary_name(name)?;
            }
        }

        Ok(())
    }
}

impl Lockfile {
    pub fn from_toml(bytes: &[u8]) -> Result<Self, LockError> {
        let text = std::str::from_utf8(bytes).map_err(|_| LockError::NotUtf8)?;
        text.parse()
    }

    pub fn validate_against_manifest(&self, manifest: &Manifest) -> Result<(), LockError> {
        if self.schema_version != LOCK_SCHEMA_VERSION {
            return Err(LockError::UnsupportedSchemaVersion {
                found: self.schema_version,
                expected: LOCK_SCHEMA_VERSION,
            });
        }

        let mut seen_names = HashSet::new();
        let mut seen_modules = HashSet::new();
        let mut by_name = HashMap::new();

        for pkg in &self.packages {
            validate_package_name(&pkg.name)
                .map_err(|_| LockError::InvalidPackageName(pkg.name.clone()))?;
            if !seen_names.insert(pkg.name.clone()) {
                return Err(LockError::DuplicatePackage(pkg.name.clone()));
            }
            if !seen_modules.insert(pkg.module.clone()) {
                return Err(LockError::DuplicateModule(pkg.module.clone()));
            }
            let expected_module = module_dir_name(&pkg.name);
            if pkg.module != expected_module {
                return Err(LockError::InvalidModuleName {
                    package: pkg.name.clone(),
                    module: pkg.module.clone(),
                    expected: expected_module,
                });
            }
            Version::parse(&pkg.version).map_err(|_| LockError::InvalidLockedVersion {
                package: pkg.name.clone(),
                version: pkg.version.clone(),
            })?;
            by_name.insert(pkg.name.clone(), pkg);
        }

        for (dep_name, dep_spec) in &manifest.dependencies {
            let locked = by_name
                .get(dep_name)
                .ok_or_else(|| LockError::MissingDirectDependency(dep_name.clone()))?;
            let version =
                Version::parse(&locked.version).map_err(|_| LockError::InvalidLockedVersion {
                    package: locked.name.clone(),
                    version: locked.version.clone(),
                })?;
            let req = semver::VersionReq::parse(dep_spec.version_req()).map_err(|_| {
                LockError::InvalidDependencyRequirement {
                    package: dep_name.clone(),
                    requirement: dep_spec.version_req().to_string(),
                }
            })?;
            if !req.matches(&version) {
                return Err(LockError::DirectDependencyMismatch {
                    package: dep_name.clone(),
                    requirement: dep_spec.version_req().to_string(),
                    locked_version: locked.version.clone(),
                });
            }
        }

        for pkg in &self.packages {
            for (dep_name, dep_req) in &pkg.dependencies {
                validate_package_name(dep_name)
                    .map_err(|_| LockError::InvalidPackageName(dep_name.clone()))?;
                let dep = by_name.get(dep_name).ok_or_else(|| {
                    LockError::MissingTransitiveDependency {
                        package: pkg.name.clone(),
                        dependency: dep_name.clone(),
                    }
                })?;
                let req = semver::VersionReq::parse(dep_req).map_err(|_| {
                    LockError::InvalidDependencyRequirement {
                        package: dep_name.clone(),
                        requirement: dep_req.clone(),
                    }
                })?;
                let version =
                    Version::parse(&dep.version).map_err(|_| LockError::InvalidLockedVersion {
                        package: dep.name.clone(),
                        version: dep.version.clone(),
                    })?;
                if !req.matches(&version) {
                    return Err(LockError::TransitiveDependencyMismatch {
                        package: pkg.name.clone(),
                        dependency: dep_name.clone(),
                        requirement: dep_req.clone(),
                        locked_version: dep.version.clone(),
                    });
                }
            }
        }

        let mut reachable = HashSet::new();
        let mut stack = Vec::new();
        for dep in manifest.dependencies.keys() {
            visit_locked(dep, &by_name, &mut reachable, &mut stack)?;
        }

        for pkg in &self.packages {
            if !reachable.contains(&pkg.name) {
                return Err(LockError::UnreachableLockedPackage(pkg.name.clone()));
            }
        }

        Ok(())
    }
}

impl FromStr for Lockfile {
    type Err = LockError;

    fn from_str(text: &str) -> Result<Self, Self::Err> {
        let lockfile: Self = toml::from_str(text)?;
        Ok(lockfile)
    }
}

impl FromStr for Manifest {
    type Err = ManifestError;

    fn from_str(text: &str) -> Result<Self, Self::Err> {
        let manifest: Self = toml::from_str(text)?;
        manifest.validate()?;
        Ok(manifest)
    }
}

#[derive(Debug, thiserror::Error)]
pub enum ManifestError {
    #[error("manifest file is not valid UTF-8")]
    NotUtf8,
    #[error("invalid TOML: {0}")]
    Toml(#[from] toml::de::Error),
    #[error("invalid package name `{0}`: must be lowercase, alphanumeric with hyphens, 1-64 chars")]
    InvalidName(String),
    #[error("invalid SemVer version `{0}`")]
    InvalidVersion(String),
    #[error("too many keywords (max 5)")]
    TooManyKeywords,
    #[error("keyword `{0}` is too long (max 20 chars)")]
    KeywordTooLong(String),
    #[error("too many categories (max 5)")]
    TooManyCategories,
    #[error("invalid URL for field `{field}`: `{value}`")]
    InvalidUrl { field: String, value: String },
    #[error("invalid version requirement `{req}` for dependency `{dep}`")]
    InvalidVersionReq { dep: String, req: String },
    #[error("invalid CLI entry: {0}")]
    InvalidCliEntry(String),
    #[error("invalid CLI binary name `{0}`")]
    InvalidCliName(String),
}

#[derive(Debug, thiserror::Error)]
pub enum LockError {
    #[error("lockfile is not valid UTF-8")]
    NotUtf8,
    #[error("invalid TOML: {0}")]
    Toml(#[from] toml::de::Error),
    #[error("unsupported dal.lock schema version `{found}` (expected {expected})")]
    UnsupportedSchemaVersion { found: u32, expected: u32 },
    #[error("invalid locked package name `{0}`")]
    InvalidPackageName(String),
    #[error("duplicate locked package `{0}`")]
    DuplicatePackage(String),
    #[error("duplicate locked module `{0}`")]
    DuplicateModule(String),
    #[error("invalid locked module `{module}` for package `{package}` (expected `{expected}`)")]
    InvalidModuleName {
        package: String,
        module: String,
        expected: String,
    },
    #[error("invalid locked version `{version}` for package `{package}`")]
    InvalidLockedVersion { package: String, version: String },
    #[error("missing direct dependency `{0}` in dal.lock")]
    MissingDirectDependency(String),
    #[error(
        "direct dependency `{package}` requires `{requirement}` but lock pins `{locked_version}`"
    )]
    DirectDependencyMismatch {
        package: String,
        requirement: String,
        locked_version: String,
    },
    #[error("missing transitive dependency `{dependency}` referenced by `{package}`")]
    MissingTransitiveDependency { package: String, dependency: String },
    #[error("invalid dependency requirement `{requirement}` for package `{package}`")]
    InvalidDependencyRequirement {
        package: String,
        requirement: String,
    },
    #[error(
        "dependency `{dependency}` of `{package}` requires `{requirement}` but lock pins `{locked_version}`"
    )]
    TransitiveDependencyMismatch {
        package: String,
        dependency: String,
        requirement: String,
        locked_version: String,
    },
    #[error("package dependency cycle detected in dal.lock: {0}")]
    DependencyCycle(String),
    #[error("unreachable locked package `{0}`")]
    UnreachableLockedPackage(String),
}

fn validate_cli_binary_name(name: &str) -> Result<(), ManifestError> {
    if name.trim().is_empty() {
        return Err(ManifestError::InvalidCliName(name.into()));
    }
    if name.contains(['/', '\\']) || name == "." || name == ".." {
        return Err(ManifestError::InvalidCliName(name.into()));
    }
    Ok(())
}

fn is_safe_relative_path(path: &std::path::Path) -> bool {
    !path.is_absolute()
        && path.components().all(|component| match component {
            std::path::Component::Normal(_) => true,
            std::path::Component::CurDir => true,
            std::path::Component::ParentDir => false,
            std::path::Component::RootDir | std::path::Component::Prefix(_) => false,
        })
}

fn module_dir_name(package: &str) -> String {
    let mut normalized = package.replace('-', "_");
    if normalized
        .chars()
        .next()
        .is_some_and(|ch| ch.is_ascii_digit())
    {
        normalized.insert(0, '_');
    }
    normalized
}

fn visit_locked(
    package: &str,
    by_name: &HashMap<String, &LockedPackage>,
    reachable: &mut HashSet<String>,
    stack: &mut Vec<String>,
) -> Result<(), LockError> {
    if let Some(pos) = stack.iter().position(|entry| entry == package) {
        let mut cycle = stack[pos..].to_vec();
        cycle.push(package.to_string());
        return Err(LockError::DependencyCycle(cycle.join(" -> ")));
    }
    if !reachable.insert(package.to_string()) {
        return Ok(());
    }

    let pkg = by_name
        .get(package)
        .ok_or_else(|| LockError::MissingDirectDependency(package.to_string()))?;
    stack.push(package.to_string());
    for dep in pkg.dependencies.keys() {
        visit_locked(dep, by_name, reachable, stack)?;
    }
    stack.pop();
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn accepts_cli_section_with_default_name() {
        let text = r#"
[package]
name = "demo"
version = "1.0.0"

[cli]
entry = "src/main.fdn"
"#;

        let manifest: Manifest = text.parse().expect("valid manifest");
        assert!(manifest.cli.is_some());
        assert_eq!(manifest.cli.as_ref().unwrap().entry, "src/main.fdn");
        assert!(manifest.cli.as_ref().unwrap().name.is_none());
    }

    #[test]
    fn rejects_invalid_cli_entry_path() {
        let text = r#"
[package]
name = "demo"
version = "1.0.0"

[cli]
entry = "../main.fdn"
"#;

        let error = text
            .parse::<Manifest>()
            .expect_err("unsafe cli entry should fail");
        assert!(error.to_string().contains("invalid CLI entry"));
    }

    #[test]
    fn validates_lockfile_against_manifest() {
        let manifest: Manifest = r#"
[package]
name = "demo"
version = "1.0.0"

[dependencies]
other-package = "^1.2"
"#
        .parse()
        .expect("valid manifest");

        let lock = Lockfile::from_toml(
            br#"
schema_version = 1

[[packages]]
name = "other-package"
module = "other_package"
version = "1.2.3"

[packages.dependencies]
leaf-package = "^2.0"

[[packages]]
name = "leaf-package"
module = "leaf_package"
version = "2.1.0"
"#,
        )
        .expect("valid lockfile");

        lock.validate_against_manifest(&manifest)
            .expect("lockfile should validate");
    }

    #[test]
    fn rejects_lockfile_with_missing_direct_dependency() {
        let manifest: Manifest = r#"
[package]
name = "demo"
version = "1.0.0"

[dependencies]
other-package = "^1.2"
"#
        .parse()
        .expect("valid manifest");

        let lock = Lockfile::from_toml(
            br#"
schema_version = 1

[[packages]]
name = "leaf-package"
module = "leaf_package"
version = "2.1.0"
"#,
        )
        .expect("valid lockfile");

        let error = lock
            .validate_against_manifest(&manifest)
            .expect_err("missing direct dep should fail");
        assert!(error.to_string().contains("missing direct dependency"));
    }
}
