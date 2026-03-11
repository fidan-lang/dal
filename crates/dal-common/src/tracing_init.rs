use tracing_subscriber::{fmt, layer::SubscriberExt, util::SubscriberInitExt, EnvFilter};

/// Initialise a global `tracing` subscriber from environment variables.
///
/// Reads `DAL_LOG_LEVEL` (default `"info"`) and `DAL_LOG_PRETTY` (default `"false"`).
/// Also respects the standard `RUST_LOG` env var for fine-grained overrides.
pub fn init() {
    let level = std::env::var("DAL_LOG_LEVEL").unwrap_or_else(|_| "info".into());
    let pretty = std::env::var("DAL_LOG_PRETTY")
        .map(|v| v.eq_ignore_ascii_case("true"))
        .unwrap_or(false);

    let filter = EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new(&level));

    if pretty {
        tracing_subscriber::registry()
            .with(filter)
            .with(fmt::layer().pretty())
            .init();
    } else {
        tracing_subscriber::registry()
            .with(filter)
            .with(fmt::layer().json())
            .init();
    }
}
