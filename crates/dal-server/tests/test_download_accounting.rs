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
