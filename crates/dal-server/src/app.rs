use crate::routes;
use crate::state::AppState;
use axum::{
    Router,
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

    let cors = CorsLayer::new()
        .allow_origin(AllowOrigin::list(allowed_origins()))
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
        .merge(routes::versions::router())
        .merge(routes::owners::router())
        .merge(routes::tokens::router())
        .merge(routes::index::router())
        .merge(routes::search::router())
        .merge(routes::health::router())
        .with_state(state)
        .layer(TraceLayer::new_for_http())
        .layer(cors)
        .layer(SetRequestIdLayer::x_request_id(MakeRequestUuid))
        .layer(PropagateRequestIdLayer::x_request_id())
        .layer(SetSensitiveRequestHeadersLayer::new([
            axum::http::header::AUTHORIZATION,
            axum::http::header::COOKIE,
        ]))
}
