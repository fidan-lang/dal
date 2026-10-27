//! Bounded, short-lived Cognito challenge state. No TOTP secrets are retained here.
use dal_common::error::DalError;
use std::{
    collections::HashMap,
    sync::Mutex,
    time::{Duration, Instant},
};
use uuid::Uuid;

pub const PENDING_TTL: Duration = Duration::from_secs(180);
pub const MAX_ATTEMPTS: u8 = 5;
const MAX_PENDING: usize = 1024;
const MAX_CONCURRENT_CHANGES: usize = 2;
pub const PENDING_COOKIE: &str = "dal_mfa_pending";
pub const SETUP_COOKIE: &str = "dal_mfa_setup";

// Intentionally no Debug/Serialize: sessions and access tokens are credentials.
pub enum PendingKind {
    Login { username: String, session: String },
    Setup { access: String },
}
pub struct Pending {
    pub user_id: Uuid,
    pub kind: PendingKind,
    epoch: Option<chrono::DateTime<chrono::Utc>>,
    expires: Instant,
    remaining: u8,
}
impl Pending {
    pub fn new(user_id: Uuid, kind: PendingKind) -> Self {
        Self {
            user_id,
            kind,
            epoch: None,
            expires: Instant::now() + PENDING_TTL,
            remaining: MAX_ATTEMPTS,
        }
    }
    pub fn at_epoch(mut self, epoch: Option<chrono::DateTime<chrono::Utc>>) -> Self {
        self.epoch = epoch;
        self
    }
    pub fn valid_for(&self, user: &dal_db::models::User) -> bool {
        self.user_id == user.id
            && self.epoch == user.session_invalid_before
            && self.expires > Instant::now()
    }
}
pub struct PendingMfa(
    Mutex<HashMap<String, Pending>>,
    std::sync::Arc<tokio::sync::Semaphore>,
);
impl Default for PendingMfa {
    fn default() -> Self {
        Self(
            Mutex::default(),
            std::sync::Arc::new(tokio::sync::Semaphore::new(MAX_CONCURRENT_CHANGES)),
        )
    }
}
impl PendingMfa {
    /// ponytail: two security changes at once bound provider I/O and held DB slots
    /// on the initial small VPS; increase only with a measured pool budget.
    pub fn change(&self) -> Result<tokio::sync::OwnedSemaphorePermit, DalError> {
        self.1
            .clone()
            .try_acquire_owned()
            .map_err(|_| DalError::RateLimited)
    }

    pub fn insert(&self, value: Pending) -> Result<String, DalError> {
        let mut pending = self.0.lock().map_err(|_| DalError::Unauthorized)?;
        pending.retain(|_, value| value.expires > Instant::now());
        if pending.len() >= MAX_PENDING {
            return Err(DalError::RateLimited);
        }
        let token = dal_auth::generate_api_token();
        pending.insert(token.hash, value);
        Ok(token.raw)
    }
    /// Atomically removes the state before any provider await: concurrent replays fail.
    pub fn take(&self, cookie: &str) -> Result<Pending, DalError> {
        let mut pending = self.0.lock().map_err(|_| DalError::Unauthorized)?;
        let mut value = pending
            .remove(&dal_auth::hash_token(cookie))
            .ok_or(DalError::Unauthorized)?;
        if value.expires <= Instant::now() || value.remaining == 0 {
            return Err(DalError::Unauthorized);
        }
        value.remaining -= 1;
        Ok(value)
    }
    pub fn retry(&self, cookie: &str, value: Pending) {
        if value.remaining > 0
            && value.expires > Instant::now()
            && let Ok(mut pending) = self.0.lock()
            && pending.len() < MAX_PENDING
        {
            pending.insert(dal_auth::hash_token(cookie), value);
        }
    }
    pub fn revoke(&self, user: Uuid) {
        if let Ok(mut pending) = self.0.lock() {
            pending.retain(|_, value| value.user_id != user);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn login(user: Uuid) -> Pending {
        Pending::new(
            user,
            PendingKind::Login {
                username: "fixture".into(),
                session: "private-session".into(),
            },
        )
    }
    #[test]
    fn one_use_bound_expiry_and_attempt_budget() {
        let state = PendingMfa::default();
        let user = Uuid::new_v4();
        let cookie = state.insert(login(user)).unwrap();
        for _ in 0..MAX_ATTEMPTS {
            let pending = state.take(&cookie).unwrap();
            assert!(state.take(&cookie).is_err());
            state.retry(&cookie, pending);
        }
        assert!(state.take(&cookie).is_err());
        let mut expired = login(user);
        expired.expires = Instant::now() - Duration::from_secs(1);
        let cookie = state.insert(expired).unwrap();
        assert!(state.take(&cookie).is_err());
        let cookie = state.insert(login(user)).unwrap();
        state.revoke(user);
        assert!(state.take(&cookie).is_err());
        assert!(state.take("forged").is_err());
    }
    #[test]
    fn storage_is_bounded_and_contains_only_hashed_handles() {
        let state = PendingMfa::default();
        let cookie = state.insert(login(Uuid::new_v4())).unwrap();
        assert!(!state.0.lock().unwrap().contains_key(&cookie));
        for _ in 1..MAX_PENDING {
            state.insert(login(Uuid::new_v4())).unwrap();
        }
        assert!(matches!(
            state.insert(login(Uuid::new_v4())),
            Err(DalError::RateLimited)
        ));
    }
}
