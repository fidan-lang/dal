use std::{net::IpAddr, num::NonZeroU32, sync::Arc, time::Duration};

use axum::{
    extract::State,
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
    map: DashMap<IpAddr, Arc<Limiter>>,
}

impl RateLimiter {
    pub fn new() -> Self {
        Self {
            map: DashMap::new(),
        }
    }

    /// Returns `true` if the request should be allowed, `false` if limited.
    pub fn check(&self, ip: IpAddr) -> bool {
        let limiter = self
            .map
            .entry(ip)
            .or_insert_with(|| {
                let quota = Quota::with_period(Duration::from_secs(60))
                    .unwrap()
                    .allow_burst(NonZeroU32::new(120).unwrap());
                Arc::new(GovRateLimiter::direct(quota))
            })
            .clone();

        limiter.check().is_ok()
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

    let ip = extract_client_ip(req.headers()).unwrap_or(IpAddr::from([127, 0, 0, 1]));
    if !state.rate.check(ip) {
        warn!(%ip, path, "auth rate limit exceeded");
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

fn extract_client_ip(headers: &HeaderMap) -> Option<IpAddr> {
    for header_name in ["x-forwarded-for", "x-real-ip"] {
        if let Some(header) = headers.get(header_name) {
            if let Ok(raw) = header.to_str() {
                if let Some(candidate) = raw.split(',').next().map(str::trim)
                    && let Ok(ip) = candidate.parse::<IpAddr>()
                {
                    return Some(ip);
                }
            }
        }
    }

    None
}
