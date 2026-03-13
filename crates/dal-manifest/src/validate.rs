use crate::ManifestError;

/// Validate a package name against the Dal naming rules:
///   - 1 to 64 characters
///   - lowercase ASCII letters, digits, and hyphens only
///   - must start and end with a letter or digit (no leading/trailing hyphens)
///   - no consecutive hyphens
pub fn validate_package_name(name: &str) -> Result<(), ManifestError> {
    if name.is_empty() || name.len() > 64 {
        return Err(ManifestError::InvalidName(name.into()));
    }
    let bytes = name.as_bytes();
    // Must start and end with alnum
    if !bytes[0].is_ascii_alphanumeric() || !bytes[bytes.len() - 1].is_ascii_alphanumeric() {
        return Err(ManifestError::InvalidName(name.into()));
    }
    // All chars must be alphanumeric or '-'
    for &b in bytes {
        if !(b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-') {
            return Err(ManifestError::InvalidName(name.into()));
        }
    }
    // No consecutive hyphens
    if name.contains("--") {
        return Err(ManifestError::InvalidName(name.into()));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn valid_names() {
        for name in ["my-pkg", "foo", "foo-bar-baz", "pkg123", "a1b2c3"] {
            validate_package_name(name).unwrap_or_else(|_| panic!("should be valid: {name}"));
        }
    }

    #[test]
    fn invalid_names() {
        for name in [
            "",
            "-starts-with-hyphen",
            "ends-with-hyphen-",
            "has--double-hyphen",
            "UPPERCASE",
            "under_score",
            "has space",
            &"a".repeat(65),
        ] {
            assert!(
                validate_package_name(name).is_err(),
                "should be invalid: {name}"
            );
        }
    }
}
