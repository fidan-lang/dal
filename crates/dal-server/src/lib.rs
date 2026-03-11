/// Re-exports every module so integration tests (and the binary) can
/// reference them via `dal_server::app`, `dal_server::state`, etc.
pub mod app;
pub mod config;
pub mod extractors;
pub mod middleware;
pub mod routes;
pub mod state;
