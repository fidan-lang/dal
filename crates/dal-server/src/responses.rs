use chrono::{DateTime, Utc};
use serde::Serialize;
use uuid::Uuid;

#[derive(Debug, Serialize)]
pub struct CurrentUserResponse {
    id: Uuid,
    username: String,
    email: String,
    display_name: Option<String>,
    avatar_url: Option<String>,
    bio: Option<String>,
    website: Option<String>,
    is_admin: bool,
    email_verified: bool,
    created_at: DateTime<Utc>,
}

impl From<dal_db::models::User> for CurrentUserResponse {
    fn from(user: dal_db::models::User) -> Self {
        Self {
            id: user.id,
            username: user.username,
            email: user.email,
            display_name: user.display_name,
            avatar_url: user.avatar_url,
            bio: user.bio,
            website: user.website,
            is_admin: user.is_admin,
            email_verified: user.email_verified,
            created_at: user.created_at,
        }
    }
}
