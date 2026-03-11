use std::{
    net::IpAddr,
    num::NonZeroU32,
    sync::Arc,
    time::Duration,
};

use dashmap::DashMap;
use governor::{
    clock::DefaultClock,
    middleware::NoOpMiddleware,
    state::{InMemoryState, NotKeyed},
    Quota, RateLimiter as GovRateLimiter,
};

type Limiter = GovRateLimiter<NotKeyed, InMemoryState, DefaultClock, NoOpMiddleware>;

/// Per-IP in-memory rate limiter backed by `governor`.
///
/// Default quota: 120 requests / 60 seconds per IP.
pub struct RateLimiter {
    map: DashMap<IpAddr, Arc<Limiter>>,
}

impl RateLimiter {
    pub fn new() -> Self {
        Self { map: DashMap::new() }
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
