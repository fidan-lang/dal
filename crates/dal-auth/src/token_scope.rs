pub const PUBLISH_NEW_SCOPE: &str = "publish:new";
pub const PUBLISH_UPDATE_SCOPE: &str = "publish:update";
pub const YANK_SCOPE: &str = "yank";
pub const OWNER_SCOPE: &str = "owner";
pub const USER_WRITE_SCOPE: &str = "user:write";

pub const DEFAULT_API_TOKEN_SCOPES: &[&str] =
    &[PUBLISH_NEW_SCOPE, PUBLISH_UPDATE_SCOPE, YANK_SCOPE];

const ALLOWED_SCOPES: &[&str] = &[
    PUBLISH_NEW_SCOPE,
    PUBLISH_UPDATE_SCOPE,
    YANK_SCOPE,
    OWNER_SCOPE,
    USER_WRITE_SCOPE,
];

pub fn normalize_scopes(scopes: &[String]) -> Result<Vec<String>, String> {
    let source = if scopes.is_empty() {
        DEFAULT_API_TOKEN_SCOPES
            .iter()
            .map(|scope| scope.to_string())
            .collect::<Vec<_>>()
    } else {
        scopes.to_vec()
    };

    let mut normalized = Vec::new();

    for scope in source {
        if !ALLOWED_SCOPES.contains(&scope.as_str()) {
            return Err(format!("unknown token scope `{scope}`"));
        }
        if !normalized.iter().any(|existing| existing == &scope) {
            normalized.push(scope);
        }
    }

    normalized.sort();
    Ok(normalized)
}

pub fn has_scope(scopes: &[String], required: &str) -> bool {
    scopes.iter().any(|scope| scope == required)
}
