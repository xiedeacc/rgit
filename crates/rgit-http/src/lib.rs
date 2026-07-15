//! rgit-http — axum HTTP server: REST API v1, git smart HTTP, git LFS,
//! and static Flutter web assets.

pub mod error;
pub mod handlers;
pub mod middleware;
pub mod router;

use rgit_core::state::AppState;

/// Run the HTTP server until failure or shutdown.
pub async fn serve(state: AppState) -> anyhow::Result<()> {
    let addr = state.config.http.bind;
    let app = router::build(state);
    let listener = tokio::net::TcpListener::bind(addr).await?;
    tracing::info!(%addr, "HTTP server listening");
    axum::serve(
        listener,
        app.into_make_service_with_connect_info::<std::net::SocketAddr>(),
    )
    .await?;
    Ok(())
}
