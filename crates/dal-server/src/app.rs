use crate::routes;
use crate::state::AppState;
use axum::{
    Router,
    extract::DefaultBodyLimit,
    http::{HeaderValue, Method, header},
    middleware,
};
use tower_http::{
    cors::{AllowHeaders, AllowOrigin, CorsLayer},
    request_id::{MakeRequestUuid, PropagateRequestIdLayer, SetRequestIdLayer},
    sensitive_headers::SetSensitiveRequestHeadersLayer,
    trace::TraceLayer,
};

fn allowed_origins() -> Vec<HeaderValue> {
    let mut origins = vec![
        "http://127.0.0.1:4173".to_string(),
        "http://localhost:4173".to_string(),
        "http://127.0.0.1:4174".to_string(),
        "http://localhost:4174".to_string(),
        "http://127.0.0.1:5173".to_string(),
        "http://localhost:5173".to_string(),
        "https://dal.fidan.dev".to_string(),
    ];

    if let Ok(extra_origins) = std::env::var("DAL_ALLOWED_ORIGINS") {
        origins.extend(
            extra_origins
                .split(',')
                .map(str::trim)
                .filter(|origin| !origin.is_empty())
                .map(ToOwned::to_owned),
        );
    }

    origins.sort();
    origins.dedup();

    origins
        .into_iter()
        .filter_map(|origin| origin.parse::<HeaderValue>().ok())
        .collect()
}

pub fn build_router(state: AppState) -> Router {
    let auth_router = routes::auth::router().route_layer(middleware::from_fn_with_state(
        state.clone(),
        crate::middleware::rate_limit::enforce_auth_rate_limit,
    ));

    let origins = allowed_origins();
    let mutation_origins = origins.clone();
    let cors = CorsLayer::new()
        .allow_origin(AllowOrigin::list(origins))
        .allow_methods([
            Method::GET,
            Method::POST,
            Method::PATCH,
            Method::PUT,
            Method::DELETE,
            Method::OPTIONS,
        ])
        .allow_headers(AllowHeaders::list([
            header::ACCEPT,
            header::AUTHORIZATION,
            header::CONTENT_TYPE,
            header::ORIGIN,
        ]))
        .allow_credentials(true);

    Router::new()
        .merge(routes::admin::router())
        .merge(auth_router)
        .merge(routes::users::router())
        .merge(routes::packages::router())
        .merge(
            routes::versions::router().layer(DefaultBodyLimit::max(
                state
                    .cfg
                    .max_upload_bytes
                    .saturating_add(64 * 1024)
                    .min(usize::MAX as u64) as usize,
            )),
        )
        .merge(routes::owners::router())
        .merge(routes::tokens::router())
        .merge(routes::index::router())
        .merge(routes::search::router())
        .merge(routes::health::router())
        .layer(middleware::from_fn_with_state(
            state.clone(),
            crate::middleware::public_reads::enforce_public_read_limit,
        ))
        .with_state(state)
        .layer(TraceLayer::new_for_http().make_span_with(request_span))
        // CORS controls response access; it does not reject CSRF mutations itself.
        // No-Origin calls remain available to CLI clients and trusted SSR refresh.
        .layer(middleware::from_fn(
            move |request: axum::extract::Request, next: axum::middleware::Next| {
                let allowed = mutation_origins.clone();
                async move {
                    if !mutation_origin_allowed(request.method(), request.headers(), &allowed) {
                        return axum::response::IntoResponse::into_response(
                            dal_common::error::DalError::Forbidden,
                        );
                    }
                    next.run(request).await
                }
            },
        ))
        .layer(cors)
        .layer(SetRequestIdLayer::x_request_id(MakeRequestUuid))
        .layer(PropagateRequestIdLayer::x_request_id())
        .layer(SetSensitiveRequestHeadersLayer::new([
            axum::http::header::AUTHORIZATION,
            axum::http::header::COOKIE,
        ]))
}

fn request_span(request: &axum::http::Request<axum::body::Body>) -> tracing::Span {
    // Verification/reset links carry credentials in their query string.
    tracing::debug_span!("http.request", method = %request.method(), path = %request.uri().path())
}

fn mutation_origin_allowed(
    method: &Method,
    headers: &axum::http::HeaderMap,
    allowed: &[HeaderValue],
) -> bool {
    matches!(*method, Method::GET | Method::HEAD | Method::OPTIONS)
        || headers
            .get(header::ORIGIN)
            .is_none_or(|origin| allowed.contains(origin))
}

#[cfg(test)]
mod origin_tests {
    use super::*;
    #[test]
    fn request_trace_never_records_query_or_header_credentials() {
        use std::sync::{Arc, Mutex};
        #[derive(Default)]
        struct Recorded(Mutex<Vec<String>>);
        struct Fields<'a>(&'a Mutex<Vec<String>>);
        impl tracing::field::Visit for Fields<'_> {
            fn record_debug(&mut self, _: &tracing::field::Field, value: &dyn std::fmt::Debug) {
                self.0.lock().unwrap().push(format!("{value:?}"));
            }
        }
        impl tracing::Subscriber for Recorded {
            fn enabled(&self, _: &tracing::Metadata<'_>) -> bool {
                true
            }
            fn new_span(&self, attributes: &tracing::span::Attributes<'_>) -> tracing::span::Id {
                attributes.record(&mut Fields(&self.0));
                tracing::span::Id::from_u64(1)
            }
            fn record(&self, _: &tracing::span::Id, values: &tracing::span::Record<'_>) {
                values.record(&mut Fields(&self.0));
            }
            fn record_follows_from(&self, _: &tracing::span::Id, _: &tracing::span::Id) {}
            fn event(&self, _: &tracing::Event<'_>) {}
            fn enter(&self, _: &tracing::span::Id) {}
            fn exit(&self, _: &tracing::span::Id) {}
        }
        let captured = Arc::new(Recorded::default());
        tracing::subscriber::with_default(captured.clone(), || {
            let request = axum::http::Request::builder()
                .uri("/auth/reset-password?token=never-log-query-marker")
                .header(header::AUTHORIZATION, "Bearer never-log-header-marker")
                .header(header::COOKIE, "token=never-log-cookie-marker")
                .body(axum::body::Body::empty())
                .unwrap();
            let _span = request_span(&request);
        });
        let values = captured.0.lock().unwrap().join(" ");
        assert!(values.contains("/auth/reset-password"));
        assert!(!values.contains("never-log"));
        assert!(!values.contains("?token"));
    }

    #[test]
    fn unsafe_browser_origins_are_rejected_while_cli_and_ssr_are_preserved() {
        let allowed = vec![HeaderValue::from_static("https://dal.fidan.dev")];
        let mut headers = axum::http::HeaderMap::new();
        assert!(mutation_origin_allowed(&Method::POST, &headers, &allowed));
        for origin in [
            "https://evil.example",
            "null",
            "https://dal.fidan.dev.evil.example",
            "https://other.fidan.dev",
        ] {
            headers.insert(header::ORIGIN, HeaderValue::from_str(origin).unwrap());
            assert!(!mutation_origin_allowed(&Method::POST, &headers, &allowed));
            assert!(!mutation_origin_allowed(
                &Method::DELETE,
                &headers,
                &allowed
            ));
            assert!(mutation_origin_allowed(&Method::GET, &headers, &allowed));
        }
        headers.insert(header::ORIGIN, allowed[0].clone());
        assert!(mutation_origin_allowed(&Method::POST, &headers, &allowed));
    }
}
