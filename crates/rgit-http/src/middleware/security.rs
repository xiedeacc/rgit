//! Browser-facing response hardening and private API cache policy.

use axum::extract::{Request, State};
use axum::http::{header, HeaderValue};
use axum::middleware::Next;
use axum::response::Response;
use rgit_core::state::AppState;

const CSP: &str = "default-src 'self'; base-uri 'self'; connect-src 'self'; font-src 'self' data:; form-action 'self'; frame-ancestors 'none'; img-src 'self' data: blob:; object-src 'none'; script-src 'self' 'unsafe-inline' 'wasm-unsafe-eval'; style-src 'self' 'unsafe-inline'; worker-src 'self' blob:";

pub async fn add_security_headers(
    State(state): State<AppState>,
    request: Request,
    next: Next,
) -> Response {
    let private_api = request.uri().path().starts_with("/api/v1/");
    let mut response = next.run(request).await;
    let headers = response.headers_mut();

    headers.insert(
        "x-content-type-options",
        HeaderValue::from_static("nosniff"),
    );
    headers.insert("x-frame-options", HeaderValue::from_static("DENY"));
    headers.insert("referrer-policy", HeaderValue::from_static("no-referrer"));
    headers.insert(
        "permissions-policy",
        HeaderValue::from_static("camera=(), microphone=(), geolocation=(), payment=()"),
    );
    headers.insert("content-security-policy", HeaderValue::from_static(CSP));

    if state.config.http.external_url.starts_with("https://") {
        headers.insert(
            "strict-transport-security",
            HeaderValue::from_static("max-age=63072000"),
        );
    }
    if private_api {
        headers.insert(header::CACHE_CONTROL, HeaderValue::from_static("no-store"));
        headers.insert(header::PRAGMA, HeaderValue::from_static("no-cache"));
    }

    response
}
