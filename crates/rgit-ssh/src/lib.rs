//! rgit-ssh — embedded SSH server (russh) for the git wire protocol.
//!
//! Only three exec commands are ever allowed:
//!   git-upload-pack / git-receive-pack / git-upload-archive
//! Authentication is public-key only, resolved against the ssh_keys table.

pub mod server;

use rgit_core::state::AppState;

/// Run the SSH server until failure or shutdown.
pub async fn serve(state: AppState) -> anyhow::Result<()> {
    if !state.config.ssh.enabled {
        tracing::info!("SSH server disabled by configuration");
        // Park forever so the select! in main doesn't treat this as failure.
        std::future::pending::<()>().await;
        return Ok(());
    }
    server::run(state).await
}
