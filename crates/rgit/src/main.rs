//! rgit — lightweight self-hosted git service (GitLab data-compatible).
//!
//! Single binary that hosts:
//!   - HTTP server (REST API + git smart HTTP + LFS + Flutter web assets)
//!   - SSH server (git-upload-pack / git-receive-pack / git-upload-archive)

use anyhow::Context;
use rgit_core::config::AppConfig;

fn main() -> anyhow::Result<()> {
    let config = AppConfig::load().context("failed to load configuration")?;
    init_tracing(&config);

    let runtime = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
        .context("failed to build tokio runtime")?;

    runtime.block_on(run(config))
}

async fn run(config: AppConfig) -> anyhow::Result<()> {
    let state = rgit_core::state::AppState::init(config).await?;

    let http = tokio::spawn(rgit_http::serve(state.clone()));
    let ssh = tokio::spawn(rgit_ssh::serve(state.clone()));

    tokio::select! {
        r = http => r??,
        r = ssh => r??,
        _ = tokio::signal::ctrl_c() => {
            tracing::info!("shutdown signal received");
        }
    }
    Ok(())
}

fn init_tracing(config: &AppConfig) {
    use tracing_subscriber::{fmt, layer::SubscriberExt, util::SubscriberInitExt, EnvFilter};

    let filter = EnvFilter::try_from_default_env()
        .unwrap_or_else(|_| EnvFilter::new(config.log.level.clone()));
    tracing_subscriber::registry()
        .with(filter)
        .with(fmt::layer())
        .init();
}
