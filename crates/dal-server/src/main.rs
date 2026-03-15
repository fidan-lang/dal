use anyhow::Context;
use dal_common::tracing_init;
use dotenvy::dotenv;
use tracing::info;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    dotenv().ok();
    tracing_init::init();

    let cfg = dal_server::config::Config::from_env().context("load config")?;
    let state = dal_server::state::AppState::build(&cfg)
        .await
        .context("build app state")?;

    let addr: std::net::SocketAddr = cfg.listen_addr.parse().context("parse listen address")?;
    info!("Dal server listening on {addr}");

    let router = dal_server::app::build_router(state);
    let listener = tokio::net::TcpListener::bind(addr).await?;
    axum::serve(listener, router)
        .with_graceful_shutdown(shutdown_signal())
        .await?;
    Ok(())
}

async fn shutdown_signal() {
    let ctrl_c = async {
        let _ = tokio::signal::ctrl_c().await;
    };

    #[cfg(unix)]
    let terminate = async {
        if let Ok(mut signal) =
            tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate())
        {
            signal.recv().await;
        }
    };

    #[cfg(not(unix))]
    let terminate = std::future::pending::<()>();

    tokio::select! {
        _ = ctrl_c => {},
        _ = terminate => {},
    }

    info!("Shutdown signal received — draining dal-server");
}
