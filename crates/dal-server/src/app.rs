use crate::routes;
use crate::state::AppState;
use axum::{http::Method, Router};
use tower_http::{
    cors::{Any, CorsLayer},
    request_id::{MakeRequestUuid, PropagateRequestIdLayer, SetRequestIdLayer},
    sensitive_headers::SetSensitiveRequestHeadersLayer,
    trace::TraceLayer,
};

pub fn build_router(state: AppState) -> Router {
    let cors = CorsLayer::new()
        .allow_origin(Any)
        .allow_methods([
            Method::GET,
            Method::POST,
            Method::PUT,
            Method::DELETE,
            Method::OPTIONS,
        ])
        .allow_headers(Any);

    Router::new()
        .merge(routes::auth::router())
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
