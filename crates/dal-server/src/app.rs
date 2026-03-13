use crate::routes;
use crate::state::AppState;
use axum::{
    Router,
    http::{HeaderValue, Method, header},
};
use tower_http::{
    cors::{AllowHeaders, AllowOrigin, CorsLayer},
    request_id::{MakeRequestUuid, PropagateRequestIdLayer, SetRequestIdLayer},
    sensitive_headers::SetSensitiveRequestHeadersLayer,
    trace::TraceLayer,
};

pub fn build_router(state: AppState) -> Router {
    let allowed_origins = [
        "http://127.0.0.1:4173",
        "http://localhost:4173",
        "http://127.0.0.1:5173",
        "http://localhost:5173",
        "https://dal.fidan.dev",
    ]
    .into_iter()
    .map(|origin| origin.parse::<HeaderValue>().unwrap())
    .collect::<Vec<_>>();

    let cors = CorsLayer::new()
        .allow_origin(AllowOrigin::list(allowed_origins))
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
