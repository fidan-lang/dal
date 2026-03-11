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
    axum::serve(listener, router).await?;
    Ok(())
}
