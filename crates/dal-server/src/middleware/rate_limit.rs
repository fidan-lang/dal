use std::{
    net::{IpAddr, SocketAddr},
    num::NonZeroU32,
    sync::Arc,
};

use axum::{
    extract::{ConnectInfo, State},
    http::{HeaderMap, Method, Request},
    middleware::Next,
    response::Response,
};
use dashmap::DashMap;
use governor::{
    Quota, RateLimiter as GovRateLimiter,
    clock::DefaultClock,
    middleware::NoOpMiddleware,
    state::{InMemoryState, NotKeyed},
};
use tracing::warn;

use dal_common::error::DalError;

use crate::state::AppState;

type Limiter = GovRateLimiter<NotKeyed, InMemoryState, DefaultClock, NoOpMiddleware>;

/// Per-IP in-memory rate limiter backed by `governor`.
///
/// Default quota: 120 requests / 60 seconds per IP.
pub struct RateLimiter {
    map: DashMap<IpAddr, (Arc<Limiter>, std::time::Instant)>,
}

impl RateLimiter {
    pub fn new() -> Self {
        Self {
            map: DashMap::new(),
        }
    }

    /// Returns `true` if the request should be allowed, `false` if limited.
    pub fn check(&self, ip: IpAddr) -> bool {
        if self.map.len() >= 10_000 {
            self.map
                .retain(|_, (_, seen)| seen.elapsed() < std::time::Duration::from_secs(600));
            if self.map.len() >= 10_000 && !self.map.contains_key(&ip) {
                return false;
            }
        }
        let mut entry = self.map.entry(ip).or_insert_with(|| {
            let quota = Quota::per_minute(NonZeroU32::new(120).unwrap());
            (
                Arc::new(GovRateLimiter::direct(quota)),
                std::time::Instant::now(),
            )
        });
        entry.1 = std::time::Instant::now();
        entry.0.check().is_ok()
    }
}

impl Default for RateLimiter {
    fn default() -> Self {
        Self::new()
    }
}

pub async fn enforce_auth_rate_limit(
    State(state): State<AppState>,
    req: Request<axum::body::Body>,
    next: Next,
) -> Result<Response, DalError> {
    let path = req.uri().path();
    if req.method() == Method::OPTIONS || !is_rate_limited_auth_path(path) {
        return Ok(next.run(req).await);
    }

    let peer = req
        .extensions()
        .get::<ConnectInfo<SocketAddr>>()
        .map(|v| v.0.ip())
        .unwrap_or(IpAddr::from([127, 0, 0, 1]));
    let ip = extract_client_ip(req.headers(), peer, &state.cfg.trusted_proxies);
    if !state.rate.check(ip) {
        warn!(path, "auth rate limit exceeded");
        return Err(DalError::RateLimited);
    }

    Ok(next.run(req).await)
}

fn is_rate_limited_auth_path(path: &str) -> bool {
    matches!(
        path,
        "/auth/register"
            | "/auth/resend-verification"
            | "/auth/login"
            | "/auth/refresh"
            | "/auth/forgot-password"
            | "/auth/reset-password"
    )
}

pub(crate) fn extract_client_ip(
    headers: &HeaderMap,
    peer: IpAddr,
    trusted_proxies: &[IpAddr],
) -> IpAddr {
    // The reverse proxy must overwrite this single-address header, not append a chain.
    if trusted_proxies.contains(&peer)
        && let Some(ip) = headers
            .get("x-forwarded-for")
            .and_then(|v| v.to_str().ok())
            .and_then(|v| v.trim().parse().ok())
    {
        return ip;
    }
    peer
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn forwarded_ip_requires_explicit_proxy_trust() {
        let peer: IpAddr = "127.0.0.1".parse().unwrap();
        let other: IpAddr = "192.0.2.1".parse().unwrap();
        let mut headers = HeaderMap::new();
        headers.insert("x-forwarded-for", "192.0.2.1".parse().unwrap());
        assert_eq!(extract_client_ip(&headers, peer, &[]), peer);
        assert_eq!(extract_client_ip(&headers, peer, &[peer]), other);
        headers.insert("x-forwarded-for", "192.0.2.1, 192.0.2.2".parse().unwrap());
        assert_eq!(extract_client_ip(&headers, peer, &[peer]), peer);
        let limiter = RateLimiter::new();
        for _ in 0..120 {
            assert!(limiter.check(peer));
        }
        assert!(!limiter.check(peer));
    }
}
