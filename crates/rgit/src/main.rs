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
    let key_count = rgit_core::auth::authorized_keys::sync_if_enabled(&state.db, &state.config.ssh)
        .await
        .context("failed to synchronize OpenSSH authorized_keys")?;
    if state.config.ssh.enabled {
        tracing::info!(key_count, "OpenSSH authorized_keys synchronized");
    }
    let (shutdown_tx, _) = tokio::sync::watch::channel(false);

    let mut http = tokio::spawn(rgit_http::serve_with_shutdown(
        state.clone(),
        shutdown_requested(shutdown_tx.subscribe()),
    ));
    let maintenance = tokio::spawn(maintenance_loop(state.clone(), shutdown_tx.subscribe()));

    let (server_error, http_done) = tokio::select! {
        result = &mut http => (Some(join_server("HTTP", result)), true),
        _ = shutdown_signal() => (None, false),
    };
    tracing::info!("shutdown requested; waiting for active operations");
    let _ = shutdown_tx.send(true);

    let graceful = async {
        if !http_done {
            let _ = (&mut http).await;
        }
        state.operations().wait_idle().await;
    };
    if tokio::time::timeout(std::time::Duration::from_secs(30), graceful)
        .await
        .is_err()
    {
        tracing::warn!(
            active = state.operations().active(),
            "shutdown grace period expired"
        );
        http.abort();
    }
    let _ = maintenance.await;
    state.db.close().await;

    match server_error {
        Some(result) => result,
        None => Ok(()),
    }
}

fn join_server(
    name: &str,
    result: Result<anyhow::Result<()>, tokio::task::JoinError>,
) -> anyhow::Result<()> {
    match result {
        Ok(Ok(())) => anyhow::bail!("{name} server stopped unexpectedly"),
        Ok(Err(error)) => Err(error).context(format!("{name} server failed")),
        Err(error) => Err(anyhow::Error::from(error)).context(format!("{name} server task failed")),
    }
}

async fn shutdown_requested(mut receiver: tokio::sync::watch::Receiver<bool>) {
    while !*receiver.borrow() {
        if receiver.changed().await.is_err() {
            return;
        }
    }
}

async fn maintenance_loop(
    state: rgit_core::state::AppState,
    mut shutdown: tokio::sync::watch::Receiver<bool>,
) {
    let mut interval = tokio::time::interval(std::time::Duration::from_secs(60 * 60));
    loop {
        tokio::select! {
            _ = interval.tick() => {
                match rgit_core::auth::session::purge_expired(&state.db).await {
                    Ok(count) if count > 0 => tracing::info!(count, "expired sessions removed"),
                    Ok(_) => {}
                    Err(error) => tracing::warn!(%error, "failed to purge expired sessions"),
                }
                if let Err(error) = rgit_core::auth::lfs_token::purge_expired(&state.db).await {
                    tracing::warn!(%error, "failed to purge expired LFS credentials");
                }
            }
            changed = shutdown.changed() => {
                if changed.is_err() || *shutdown.borrow() {
                    return;
                }
            }
        }
    }
}

async fn shutdown_signal() {
    #[cfg(unix)]
    {
        let mut terminate =
            tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate())
                .expect("install SIGTERM handler");
        tokio::select! {
            _ = tokio::signal::ctrl_c() => {}
            _ = terminate.recv() => {}
        }
    }
    #[cfg(not(unix))]
    {
        let _ = tokio::signal::ctrl_c().await;
    }
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
