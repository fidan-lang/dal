use std::{
    collections::HashMap,
    net::{IpAddr, SocketAddr},
    num::NonZeroU32,
    sync::Mutex,
    time::{Duration, Instant},
};

use axum::{
    extract::{ConnectInfo, State},
    http::{HeaderMap, Method, Request},
    middleware::Next,
    response::Response,
};
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

const MAX_TRACKED_IPS: usize = 10_000;
const IDLE_RETENTION: Duration = Duration::from_secs(600);
const CLEANUP_INTERVAL: Duration = Duration::from_secs(60);

struct RateLimitState {
    map: HashMap<IpAddr, (Limiter, Instant)>,
    next_cleanup: Instant,
}

/// Per-IP in-memory rate limiter backed by `governor`.
///
/// Default quota: 120 requests / 60 seconds per IP.
pub struct RateLimiter {
    state: Mutex<RateLimitState>,
    quota: NonZeroU32,
}

impl RateLimiter {
    pub fn new() -> Self {
        Self::with_quota(NonZeroU32::new(120).unwrap())
    }

    pub fn with_quota(quota: NonZeroU32) -> Self {
        Self {
            state: Mutex::new(RateLimitState {
                map: HashMap::new(),
                next_cleanup: Instant::now() + CLEANUP_INTERVAL,
            }),
            quota,
        }
    }

    /// Returns `true` if the request should be allowed, `false` if limited.
    pub fn check(&self, ip: IpAddr) -> bool {
        // ponytail: one short in-memory lock makes admission and the hard cap atomic;
        // shard it only if measured traffic warrants coordinated shard budgets.
        let Ok(mut state) = self.state.lock() else {
            return false; // Poisoned state must fail closed.
        };
        let now = Instant::now();
        if let Some((limiter, seen)) = state.map.get_mut(&ip) {
            *seen = now;
            return limiter.check().is_ok();
        }
        // Never scan the entire table on each rejected new address.
        if now >= state.next_cleanup {
            state
                .map
                .retain(|_, (_, seen)| now.duration_since(*seen) < IDLE_RETENTION);
            state.next_cleanup = now + CLEANUP_INTERVAL;
        }
        if state.map.len() >= MAX_TRACKED_IPS {
            return false;
        }
        let entry = state
            .map
            .entry(ip)
            .or_insert_with(|| (GovRateLimiter::direct(Quota::per_minute(self.quota)), now));
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
    fn concurrent_new_addresses_never_exceed_the_state_budget() {
        let limiter = RateLimiter::new();
        for i in 0..9_999u32 {
            assert!(limiter.check(std::net::Ipv4Addr::from(i).into()));
        }
        let barrier = std::sync::Barrier::new(32);
        std::thread::scope(|scope| {
            for i in 10_000..10_032u32 {
                let limiter = &limiter;
                let barrier = &barrier;
                scope.spawn(move || {
                    barrier.wait();
                    limiter.check(std::net::Ipv4Addr::from(i).into());
                });
            }
        });
        assert!(
            limiter.state.lock().unwrap().map.len() <= MAX_TRACKED_IPS,
            "concurrent insertion exceeded the IP budget"
        );
    }

    #[test]
    fn shared_nat_uses_one_quota_and_idle_cleanup_is_bounded() {
        let limiter = RateLimiter::with_quota(NonZeroU32::new(1).unwrap());
        let nat: IpAddr = "192.0.2.1".parse().unwrap();
        assert!(limiter.check(nat));
        for _ in 0..32 {
            assert!(!limiter.check(nat));
        }
        let other: IpAddr = "192.0.2.2".parse().unwrap();
        assert!(limiter.check(other));
        {
            let mut state = limiter.state.lock().unwrap();
            state.map.get_mut(&other).unwrap().1 =
                Instant::now() - IDLE_RETENTION - Duration::from_secs(1);
        }
        assert!(limiter.check("192.0.2.3".parse().unwrap()));
        assert!(
            limiter.state.lock().unwrap().map.contains_key(&other),
            "do not rescan the table for each new IP"
        );
        limiter.state.lock().unwrap().next_cleanup = Instant::now();
        assert!(limiter.check("192.0.2.4".parse().unwrap()));
        assert!(!limiter.state.lock().unwrap().map.contains_key(&other));
        assert!(
            !limiter.check(nat),
            "maintenance must not reset an active address quota"
        );
    }

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
