mod common;
use axum::{body::Body, extract::ConnectInfo, http::Request};
use dal_db::queries;
use dal_server::{app::build_router, state::AppState};
use tower::ServiceExt;
use uuid::Uuid;

async fn fixture(state: &AppState) -> (Uuid, Uuid, String) {
    let id = Uuid::new_v4();
    let name = format!("telemetry-{}", id.simple());
    queries::users::create(
        &state.db,
        id,
        &name,
        &format!("{name}@example.test"),
        &id.to_string(),
        None,
    )
    .await
    .unwrap();
    queries::packages::create(
        &state.db,
        queries::packages::NewPackage {
            id,
            name: &name,
            description: None,
            repository: None,
            homepage: None,
            license: None,
            keywords: &[],
            categories: &[],
        },
    )
    .await
    .unwrap();
    let version = Uuid::new_v4();
    queries::versions::create(
        &state.db,
        queries::versions::NewPackageVersion {
            id: version,
            package_id: id,
            version: "0.1.0",
            checksum: &"a".repeat(64),
            size_bytes: 1,
            s3_key: "test/telemetry.tar.gz",
            readme: None,
            manifest: serde_json::json!({}),
            published_by: id,
        },
    )
    .await
    .unwrap();
    (
        id,
        version,
        format!("/packages/{name}/versions/0.1.0/download"),
    )
}

#[tokio::test]
async fn public_download_preserves_signed_access_without_caching_the_capability() {
    let state = AppState::build(&common::test_config()).await.unwrap();
    let (_, version, uri) = fixture(&state).await;
    let key = format!("test/download-contract/{version}.tar.gz");
    let payload = b"public package download fixture";
    state
        .storage
        .upload(&key, payload.to_vec(), "application/gzip")
        .await
        .unwrap();
    sqlx::query("UPDATE package_versions SET s3_key=$2 WHERE id=$1")
        .bind(version)
        .bind(&key)
        .execute(&state.db)
        .await
        .unwrap();
    // No bearer token, cookie or Origin is required for public package access.
    let response = build_router(state.clone())
        .oneshot(Request::builder().uri(uri).body(Body::empty()).unwrap())
        .await
        .unwrap();
    assert_eq!(response.status(), 307);
    assert_eq!(
        response
            .headers()
            .get("cache-control")
            .expect("signed redirects must be uncacheable"),
        "private, no-store"
    );
    assert_eq!(
        response
            .headers()
            .get("referrer-policy")
            .expect("signed redirects must suppress referrers"),
        "no-referrer"
    );
    let signed = reqwest::Url::parse(
        response
            .headers()
            .get("location")
            .unwrap()
            .to_str()
            .unwrap(),
    )
    .unwrap();
    let query = signed
        .query_pairs()
        .collect::<std::collections::HashMap<_, _>>();
    assert_eq!(query.get("X-Amz-Expires").map(|v| v.as_ref()), Some("60"));
    assert!(query.contains_key("X-Amz-Signature"));
    assert!(signed.path().ends_with(&key));
    assert_eq!(signed.host_str(), Some("127.0.0.1"));
    let archive = reqwest::get(signed).await.unwrap();
    assert!(archive.status().is_success());
    assert_eq!(archive.bytes().await.unwrap().as_ref(), payload);
    state.storage.delete(&key).await.unwrap();
}

#[tokio::test]
async fn download_telemetry_ignores_spoofed_forwarding_and_caps_user_agent() {
    let state = AppState::build(&common::test_config()).await.unwrap();
    let (_, version, uri) = fixture(&state).await;
    let request = Request::builder()
        .uri(uri)
        .extension(ConnectInfo(
            "192.0.2.10:1234".parse::<std::net::SocketAddr>().unwrap(),
        ))
        .header("x-forwarded-for", "203.0.113.99")
        .header("user-agent", "x".repeat(16000))
        .body(Body::empty())
        .unwrap();
    assert_eq!(
        build_router(state.clone())
            .oneshot(request)
            .await
            .unwrap()
            .status(),
        307
    );
    // The old implementation detached its write; wait only for visibility, never retry assertions.
    let mut result = None;
    for _ in 0..100 {
        result = sqlx::query_as::<_, (String, Option<String>)>(
            "SELECT ip_hash,user_agent FROM download_logs WHERE version_id=$1",
        )
        .bind(version)
        .fetch_optional(&state.db)
        .await
        .unwrap();
        if result.is_some() {
            break;
        }
        tokio::time::sleep(std::time::Duration::from_millis(10)).await;
    }
    let (ip, ua) = result.expect("one admitted download must be accounted");
    assert_eq!(
        ip,
        dal_auth::api_token::hash_ip("192.0.2.10"),
        "client headers must not impersonate peer identity"
    );
    assert!(
        ua.unwrap().len() <= 512,
        "persisted user agent must be bounded"
    );
}

async fn counts(state: &AppState, package: Uuid, version: Uuid) -> (i64, i64, i64) {
    let total: i64 = sqlx::query_scalar("SELECT downloads FROM packages WHERE id=$1")
        .bind(package)
        .fetch_one(&state.db)
        .await
        .unwrap();
    let per_version: i64 = sqlx::query_scalar("SELECT downloads FROM package_versions WHERE id=$1")
        .bind(version)
        .fetch_one(&state.db)
        .await
        .unwrap();
    let logs: i64 = sqlx::query_scalar("SELECT count(*) FROM download_logs WHERE version_id=$1")
        .bind(version)
        .fetch_one(&state.db)
        .await
        .unwrap();
    (total, per_version, logs)
}

#[tokio::test]
async fn saturated_accounting_drops_a_burst_without_blocking_downloads() {
    let state = AppState::build(&common::test_config()).await.unwrap();
    let (package, version, uri) = fixture(&state).await;
    let permits = state
        .accounting_slots
        .acquire_many(dal_server::state::ACCOUNTING_CONCURRENCY as u32)
        .await
        .unwrap();
    let mut requests = tokio::task::JoinSet::new();
    for _ in 0..32 {
        let router = build_router(state.clone());
        let uri = uri.clone();
        requests.spawn(async move {
            router
                .oneshot(Request::builder().uri(uri).body(Body::empty()).unwrap())
                .await
                .unwrap()
                .status()
        });
    }
    while let Some(status) = requests.join_next().await {
        assert_eq!(status.unwrap(), 307);
    }
    drop(permits);
    assert_eq!(
        counts(&state, package, version).await,
        (0, 0, 0),
        "saturation must not create a detached backlog"
    );
    assert_eq!(
        build_router(state.clone())
            .oneshot(Request::builder().uri(uri).body(Body::empty()).unwrap())
            .await
            .unwrap()
            .status(),
        307
    );
    assert_eq!(counts(&state, package, version).await, (1, 1, 1));
}

#[tokio::test]
async fn slow_database_times_out_and_cancels_all_download_accounting() {
    let state = AppState::build(&common::test_config()).await.unwrap();
    let (package, version, uri) = fixture(&state).await;
    let mut lock = state.db.begin().await.unwrap();
    sqlx::query("SELECT id FROM package_versions WHERE id=$1 FOR UPDATE")
        .bind(version)
        .execute(&mut *lock)
        .await
        .unwrap();
    let result = tokio::time::timeout(
        std::time::Duration::from_secs(2),
        build_router(state.clone())
            .oneshot(Request::builder().uri(uri).body(Body::empty()).unwrap()),
    )
    .await;
    lock.rollback().await.unwrap();
    assert_eq!(
        result
            .expect("telemetry must not wait indefinitely for SQL locks")
            .unwrap()
            .status(),
        307
    );
    assert_eq!(
        counts(&state, package, version).await,
        (0, 0, 0),
        "timed-out accounting must not later mutate counters"
    );
    assert_eq!(
        state.accounting_slots.available_permits(),
        dal_server::state::ACCOUNTING_CONCURRENCY
    );
}

#[tokio::test]
async fn failed_database_accounting_rolls_back_both_counters_and_keeps_redirect() {
    let state = AppState::build(&common::test_config()).await.unwrap();
    let (package, version, uri) = fixture(&state).await;
    sqlx::query("UPDATE packages SET downloads=$1 WHERE id=$2")
        .bind(i64::MAX)
        .bind(package)
        .execute(&state.db)
        .await
        .unwrap();
    let response = build_router(state.clone())
        .oneshot(Request::builder().uri(uri).body(Body::empty()).unwrap())
        .await
        .unwrap();
    assert_eq!(response.status(), 307);
    assert_eq!(counts(&state, package, version).await, (i64::MAX, 0, 0));
}

#[tokio::test]
async fn forwarding_is_used_only_with_real_explicitly_trusted_peer() {
    let mut cfg = common::test_config();
    cfg.trusted_proxies = vec!["192.0.2.10".parse().unwrap()];
    let state = AppState::build(&cfg).await.unwrap();
    for (peer, forwarded, expected) in [
        (None, "203.0.113.99", "unknown"),
        (Some("192.0.2.10:1234"), "203.0.113.99", "203.0.113.99"),
        (
            Some("192.0.2.10:1234"),
            "203.0.113.99, 203.0.113.100",
            "192.0.2.10",
        ),
    ] {
        let (_, version, uri) = fixture(&state).await;
        let mut request = Request::builder()
            .uri(uri)
            .header("x-forwarded-for", forwarded)
            .body(Body::empty())
            .unwrap();
        if let Some(peer) = peer {
            request
                .extensions_mut()
                .insert(ConnectInfo(peer.parse::<std::net::SocketAddr>().unwrap()));
        }
        assert_eq!(
            build_router(state.clone())
                .oneshot(request)
                .await
                .unwrap()
                .status(),
            307
        );
        let actual: String =
            sqlx::query_scalar("SELECT ip_hash FROM download_logs WHERE version_id=$1")
                .bind(version)
                .fetch_one(&state.db)
                .await
                .unwrap();
        assert_eq!(actual, dal_auth::api_token::hash_ip(expected));
    }
}

#[tokio::test]
async fn cancellation_drops_accounting_work_and_releases_its_permit() {
    let state = AppState::build(&common::test_config()).await.unwrap();
    struct Cancelled(std::sync::Arc<std::sync::atomic::AtomicBool>);
    impl Drop for Cancelled {
        fn drop(&mut self) {
            self.0.store(true, std::sync::atomic::Ordering::SeqCst);
        }
    }
    let cancelled = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
    let marker = Cancelled(cancelled.clone());
    let operation = async move {
        let _marker = marker;
        std::future::pending::<Result<(), dal_common::error::DalError>>().await
    };
    {
        let request = state.best_effort_accounting(operation);
        tokio::pin!(request);
        assert!(matches!(
            std::future::poll_fn(|cx| std::task::Poll::Ready(request.as_mut().poll(cx))).await,
            std::task::Poll::Pending
        ));
        assert_eq!(
            state.accounting_slots.available_permits(),
            dal_server::state::ACCOUNTING_CONCURRENCY - 1
        );
    }
    assert!(cancelled.load(std::sync::atomic::Ordering::SeqCst));
    assert_eq!(
        state.accounting_slots.available_permits(),
        dal_server::state::ACCOUNTING_CONCURRENCY
    );
}

#[tokio::test]
async fn stream_download_never_exposes_reusable_storage_credentials() {
    let mut cfg = common::test_config();
    cfg.public_reads.mode = dal_server::config::DownloadMode::Stream;
    let state = AppState::build(&cfg).await.unwrap();
    let (_, version, uri) = fixture(&state).await;
    let key = format!("test/stream-contract/{version}.tar.gz");
    let payload = b"anonymous streamed package";
    state
        .storage
        .upload(&key, payload.to_vec(), "application/gzip")
        .await
        .unwrap();
    sqlx::query("UPDATE package_versions SET s3_key=$2,size_bytes=$3 WHERE id=$1")
        .bind(version)
        .bind(&key)
        .bind(payload.len() as i64)
        .execute(&state.db)
        .await
        .unwrap();
    let router = build_router(state.clone());
    let response = router
        .clone()
        .oneshot(Request::builder().uri(&uri).body(Body::empty()).unwrap())
        .await
        .unwrap();
    assert_eq!(response.status(), 200);
    assert!(
        response.headers().get("location").is_none(),
        "no signed R2 URL may escape the controlled download route"
    );
    assert_eq!(response.headers()["content-type"], "application/gzip");
    assert!(
        response.headers()["cache-control"]
            .to_str()
            .unwrap()
            .contains("immutable")
    );
    let etag = response.headers()["etag"].clone();
    assert_eq!(
        axum::body::to_bytes(response.into_body(), 1024)
            .await
            .unwrap()
            .as_ref(),
        payload
    );
    let cached = router
        .oneshot(
            Request::builder()
                .uri(&uri)
                .header("if-none-match", etag)
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(cached.status(), 304);
    state.storage.delete(&key).await.unwrap();
}

#[tokio::test]
async fn public_read_limits_apply_without_an_account_and_cannot_be_spoofed() {
    let mut cfg = common::test_config();
    cfg.public_reads.download_requests_per_minute = 1;
    cfg.public_reads.metadata_requests_per_minute = 1;
    let state = AppState::build(&cfg).await.unwrap();
    let (_, _, uri) = fixture(&state).await;
    let router = build_router(state);
    let first = router
        .clone()
        .oneshot(Request::builder().uri(&uri).body(Body::empty()).unwrap())
        .await
        .unwrap();
    assert_eq!(first.status(), 307); // Explicit legacy mode remains compatible.
    let repeated = router
        .clone()
        .oneshot(
            Request::builder()
                .uri(format!("{uri}?cache_bust=untrusted"))
                .header("x-forwarded-for", "192.0.2.99")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(repeated.status(), 429);
    assert!(repeated.headers().get("retry-after").is_some());
    let metadata = "/index/nonexistent-read-limit-probe";
    let first = router
        .clone()
        .oneshot(
            Request::builder()
                .uri(metadata)
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(
        first.status(),
        404,
        "metadata and downloads need independent quotas"
    );
    assert_eq!(
        router
            .clone()
            .oneshot(
                Request::builder()
                    .uri(metadata)
                    .body(Body::empty())
                    .unwrap()
            )
            .await
            .unwrap()
            .status(),
        429
    );
    assert_ne!(
        router
            .oneshot(
                Request::builder()
                    .uri("/health")
                    .body(Body::empty())
                    .unwrap()
            )
            .await
            .unwrap()
            .status(),
        429
    );
}

#[tokio::test]
async fn streaming_holds_global_capacity_until_the_client_finishes_or_disconnects() {
    let mut cfg = common::test_config();
    cfg.public_reads.mode = dal_server::config::DownloadMode::Stream;
    cfg.public_reads.concurrent_downloads = 1;
    let state = AppState::build(&cfg).await.unwrap();
    let (_, version, uri) = fixture(&state).await;
    let key = format!("test/admission/{version}.tar.gz");
    state
        .storage
        .upload(&key, b"x".to_vec(), "application/gzip")
        .await
        .unwrap();
    sqlx::query("UPDATE package_versions SET s3_key=$2 WHERE id=$1")
        .bind(version)
        .bind(&key)
        .execute(&state.db)
        .await
        .unwrap();
    let router = build_router(state.clone());
    let first = router
        .clone()
        .oneshot(Request::builder().uri(&uri).body(Body::empty()).unwrap())
        .await
        .unwrap();
    assert_eq!(first.status(), 200);
    assert_eq!(state.download_slots.available_permits(), 0);
    let blocked = router
        .clone()
        .oneshot(Request::builder().uri(&uri).body(Body::empty()).unwrap())
        .await
        .unwrap();
    assert_eq!(blocked.status(), 429);
    assert!(blocked.headers().get("retry-after").is_some());
    let head = router
        .clone()
        .oneshot(
            Request::builder()
                .method("HEAD")
                .uri(&uri)
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(head.status(), 200);
    assert_eq!(head.headers()["content-length"], "1");
    drop(first);
    assert_eq!(state.download_slots.available_permits(), 1);
    let next = router
        .oneshot(Request::builder().uri(&uri).body(Body::empty()).unwrap())
        .await
        .unwrap();
    assert_eq!(next.status(), 200);
    assert_eq!(
        axum::body::to_bytes(next.into_body(), 16)
            .await
            .unwrap()
            .as_ref(),
        b"x"
    );
    state.storage.delete(&key).await.unwrap();
}

#[tokio::test]
async fn concurrent_metadata_is_bounded_without_blocking_health_or_leaking_slots() {
    let mut cfg = common::test_config();
    cfg.public_reads.concurrent_metadata_requests = 1;
    let state = AppState::build(&cfg).await.unwrap();
    let started = std::sync::Arc::new(tokio::sync::Notify::new());
    let release = std::sync::Arc::new(tokio::sync::Notify::new());
    let router = axum::Router::new()
        .route(
            "/index/slow",
            axum::routing::get({
                let started = started.clone();
                let release = release.clone();
                move || {
                    let started = started.clone();
                    let release = release.clone();
                    async move {
                        started.notify_one();
                        release.notified().await;
                        "metadata"
                    }
                }
            }),
        )
        .route("/health", axum::routing::get(|| async { "healthy" }))
        .layer(axum::middleware::from_fn_with_state(
            state,
            dal_server::middleware::public_reads::enforce_public_read_limit,
        ));
    let first = tokio::spawn(
        router.clone().oneshot(
            Request::builder()
                .uri("/index/slow")
                .body(Body::empty())
                .unwrap(),
        ),
    );
    tokio::time::timeout(std::time::Duration::from_secs(5), started.notified())
        .await
        .unwrap();
    let blocked = router
        .clone()
        .oneshot(
            Request::builder()
                .uri("/index/slow?cache_bust=1")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(blocked.status(), 429);
    assert_eq!(blocked.headers()["retry-after"], "1");
    assert_eq!(blocked.headers()["cache-control"], "no-store");
    assert_eq!(
        router
            .clone()
            .oneshot(
                Request::builder()
                    .uri("/health")
                    .body(Body::empty())
                    .unwrap()
            )
            .await
            .unwrap()
            .status(),
        200
    );
    first.abort(); // Cancellation must release metadata admission too.
    let _ = first.await;
    release.notify_one();
    let next = router
        .oneshot(
            Request::builder()
                .uri("/index/slow")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(next.status(), 200);
}
