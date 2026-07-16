//! Static Flutter web assets with SPA fallback to index.html.

use axum::extract::State;
use axum::http::{header, StatusCode, Uri};
use axum::response::{IntoResponse, Response};
use rgit_core::state::AppState;

pub async fn spa(State(state): State<AppState>, uri: Uri) -> Response {
    let root = &state.config.web.static_dir;
    let rel = uri.path().trim_start_matches('/');

    // Path-traversal guard: only serve simple relative paths below root.
    let candidate = if rel.is_empty() { "index.html" } else { rel };
    let safe = !candidate
        .split('/')
        .any(|seg| seg.is_empty() || seg == "." || seg == "..");
    let path = root.join(candidate);

    let served = if safe && path.is_file() {
        path
    } else {
        root.join("index.html")
    };
    match tokio::fs::read(&served).await {
        Ok(bytes) => {
            let mime = mime_guess::from_path(&served).first_or_octet_stream();
            (
                [
                    (header::CONTENT_TYPE, mime.essence_str().to_string()),
                    (header::X_CONTENT_TYPE_OPTIONS, "nosniff".to_string()),
                ],
                bytes,
            )
                .into_response()
        }
        Err(_) => (
            StatusCode::NOT_FOUND,
            "web assets not found — build the Flutter app and set [web].static_dir\n",
        )
            .into_response(),
    }
}
