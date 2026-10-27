use crate::{
    middleware::rate_limit::{RateLimiter, extract_client_ip},
    state::AppState,
};
use axum::{
    extract::{ConnectInfo, State},
    http::{Method, Request, header},
    middleware::Next,
    response::{IntoResponse, Response},
};
use serde::Deserialize;
use std::num::NonZeroU32;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum DownloadMode {
    Stream,
    Redirect,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PublicReadConfig {
    pub mode: DownloadMode,
    pub metadata_requests_per_minute: u32,
    pub download_requests_per_minute: u32,
    pub concurrent_downloads: usize,
    pub concurrent_metadata_requests: usize,
    pub download_timeout_seconds: u64,
    pub archive_cache_seconds: u32,
}
impl Default for PublicReadConfig {
    fn default() -> Self {
        serde_json::from_str(include_str!("../../../../config/public-read-policy.json"))
            .expect("checked-in public read policy must deserialize")
    }
}
impl PublicReadConfig {
    pub fn from_env() -> anyhow::Result<Self> {
        let mut value = Self::default();
        if let Ok(mode) = std::env::var("DAL_DOWNLOAD_MODE") {
            value.mode = match mode.as_str() {
                "stream" => DownloadMode::Stream,
                "redirect" => DownloadMode::Redirect,
                _ => anyhow::bail!("DAL_DOWNLOAD_MODE must be stream or redirect"),
            };
        }
        macro_rules! setting {
            ($name:literal, $field:ident) => {
                if let Ok(raw) = std::env::var($name) {
                    value.$field = raw.parse().map_err(|_| {
                        anyhow::anyhow!(concat!($name, " must be a positive integer"))
                    })?;
                }
            };
        }
        setting!(
            "DAL_METADATA_REQUESTS_PER_MINUTE",
            metadata_requests_per_minute
        );
        setting!(
            "DAL_DOWNLOAD_REQUESTS_PER_MINUTE",
            download_requests_per_minute
        );
        setting!("DAL_CONCURRENT_DOWNLOADS", concurrent_downloads);
        setting!(
            "DAL_CONCURRENT_METADATA_REQUESTS",
            concurrent_metadata_requests
        );
        setting!("DAL_DOWNLOAD_TIMEOUT_SECONDS", download_timeout_seconds);
        value.validate()?;
        Ok(value)
    }
    pub fn validate(&self) -> anyhow::Result<()> {
        anyhow::ensure!(
            (1..=60_000).contains(&self.metadata_requests_per_minute),
            "metadata quota must be 1..60000 per minute"
        );
        anyhow::ensure!(
            (1..=6_000).contains(&self.download_requests_per_minute),
            "download quota must be 1..6000 per minute"
        );
        anyhow::ensure!(
            (1..=256).contains(&self.concurrent_metadata_requests),
            "metadata concurrency must be 1..256"
        );
        anyhow::ensure!(
            (1..=256).contains(&self.concurrent_downloads),
            "download concurrency must be 1..256"
        );
        anyhow::ensure!(
            (1..=600).contains(&self.download_timeout_seconds),
            "download timeout must be 1..600 seconds"
        );
        anyhow::ensure!(
            self.archive_cache_seconds > 0,
            "immutable archive cache lifetime must be positive"
        );
        Ok(())
    }
}

pub struct PublicReadLimits {
    metadata: RateLimiter,
    downloads: RateLimiter,
    metadata_slots: tokio::sync::Semaphore,
}
impl PublicReadLimits {
    pub fn new(cfg: &PublicReadConfig) -> Self {
        Self {
            metadata_slots: tokio::sync::Semaphore::new(cfg.concurrent_metadata_requests),
            metadata: RateLimiter::with_quota(
                NonZeroU32::new(cfg.metadata_requests_per_minute)
                    .expect("validated metadata quota"),
            ),
            downloads: RateLimiter::with_quota(
                NonZeroU32::new(cfg.download_requests_per_minute)
                    .expect("validated download quota"),
            ),
        }
    }
}
fn read_class(method: &Method, path: &str) -> Option<bool> {
    if !matches!(*method, Method::GET | Method::HEAD) {
        return None;
    }
    if path.starts_with("/packages/") && path.ends_with("/download") {
        return Some(true);
    }
    ["/packages", "/index", "/search"]
        .into_iter()
        .any(|prefix| {
            path == prefix
                || path
                    .strip_prefix(prefix)
                    .is_some_and(|rest| rest.starts_with('/'))
        })
        .then_some(false)
}
pub async fn enforce_public_read_limit(
    State(state): State<AppState>,
    request: Request<axum::body::Body>,
    next: Next,
) -> Response {
    let Some(download) = read_class(request.method(), request.uri().path()) else {
        return next.run(request).await;
    };
    let peer = request
        .extensions()
        .get::<ConnectInfo<std::net::SocketAddr>>()
        .map(|v| v.0.ip())
        .unwrap_or(std::net::Ipv4Addr::LOCALHOST.into());
    let ip = extract_client_ip(request.headers(), peer, &state.cfg.trusted_proxies);
    let (limiter, quota) = if download {
        (
            &state.public_reads.downloads,
            state.cfg.public_reads.download_requests_per_minute,
        )
    } else {
        (
            &state.public_reads.metadata,
            state.cfg.public_reads.metadata_requests_per_minute,
        )
    };
    if !limiter.check(ip) {
        let mut response = dal_common::error::DalError::RateLimited.into_response();
        response.headers_mut().insert(
            header::RETRY_AFTER,
            60u32
                .div_ceil(quota)
                .to_string()
                .parse()
                .expect("integer header"),
        );
        response.headers_mut().insert(
            header::CACHE_CONTROL,
            header::HeaderValue::from_static("no-store"),
        );
        return response;
    }
    let _metadata_slot = if !download {
        match state.public_reads.metadata_slots.try_acquire() {
            Ok(slot) => Some(slot),
            Err(_) => {
                let mut response = dal_common::error::DalError::RateLimited.into_response();
                response
                    .headers_mut()
                    .insert(header::RETRY_AFTER, header::HeaderValue::from_static("1"));
                response.headers_mut().insert(
                    header::CACHE_CONTROL,
                    header::HeaderValue::from_static("no-store"),
                );
                return response;
            }
        }
    } else {
        None
    };
    let mut response = next.run(request).await;
    if response.status() == axum::http::StatusCode::TOO_MANY_REQUESTS
        && !response.headers().contains_key(header::RETRY_AFTER)
    {
        response
            .headers_mut()
            .insert(header::RETRY_AFTER, header::HeaderValue::from_static("1"));
    }
    if response.status().is_client_error() || response.status().is_server_error() {
        response.headers_mut().insert(
            header::CACHE_CONTROL,
            header::HeaderValue::from_static("no-store"),
        );
    }
    response
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn policy_is_valid_and_only_public_read_paths_are_classified() {
        PublicReadConfig::default().validate().unwrap();
        assert_eq!(
            read_class(&Method::GET, "/packages/x/versions/1.0.0/download"),
            Some(true)
        );
        assert_eq!(read_class(&Method::HEAD, "/index/x"), Some(false));
        for path in ["/health", "/auth/login", "/packages-elsewhere"] {
            assert_eq!(read_class(&Method::GET, path), None);
        }
        assert_eq!(read_class(&Method::POST, "/packages/x/publish"), None);
    }
    #[test]
    fn invalid_quotas_fail_closed() {
        for cfg in [
            PublicReadConfig {
                download_requests_per_minute: 0,
                ..Default::default()
            },
            PublicReadConfig {
                concurrent_downloads: 0,
                ..Default::default()
            },
            PublicReadConfig {
                concurrent_metadata_requests: 0,
                ..Default::default()
            },
            PublicReadConfig {
                download_timeout_seconds: 601,
                ..Default::default()
            },
        ] {
            assert!(cfg.validate().is_err());
        }
    }
}
