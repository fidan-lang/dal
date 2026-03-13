mod validate;

use semver::Version;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::str::FromStr;
use url::Url;

pub use validate::validate_package_name;

/// Parsed and validated `dal.toml` manifest.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Manifest {
    pub package: PackageMeta,
    #[serde(default)]
    pub dependencies: HashMap<String, DependencySpec>,
    #[serde(default)]
    pub dev_dependencies: HashMap<String, DependencySpec>,
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

        Ok(())
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
}
