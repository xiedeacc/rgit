//! REST API v1 (DESIGN.md §10).

pub mod admin;
pub mod groups;
pub mod keys;
pub mod members;
pub mod projects;
pub mod repo_browse;
pub mod session;
pub mod tokens;
pub mod user;

use axum::extract::State;
use axum::http::{HeaderValue, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::Json;
use rgit_core::state::AppState;
use serde::{Deserialize, Serialize};

#[derive(Debug, Serialize)]
pub struct Status {
    pub uptime_seconds: u64,
}

pub async fn status(State(state): State<AppState>) -> Json<Status> {
    let uptime_seconds = state.started_at.elapsed().unwrap_or_default().as_secs();
    Json(Status { uptime_seconds })
}

pub async fn not_found() -> Response {
    (
        StatusCode::NOT_FOUND,
        Json(serde_json::json!({
            "error": "not_found",
            "message": "API endpoint not found"
        })),
    )
        .into_response()
}

pub fn paginated_json<T: Serialize>(items: T, total: i64) -> Response {
    let mut response = Json(items).into_response();
    let value = HeaderValue::from_str(&total.max(0).to_string())
        .expect("non-negative integer is a valid header value");
    response.headers_mut().insert("x-total", value);
    response
}

/// Common pagination query (?page=1&per_page=20, capped).
#[derive(Debug, Clone, Copy, Deserialize)]
pub struct Pagination {
    #[serde(default = "default_page")]
    pub page: u32,
    #[serde(default = "default_per_page")]
    pub per_page: u32,
}

fn default_page() -> u32 {
    1
}
fn default_per_page() -> u32 {
    20
}

impl Pagination {
    pub fn from_options(page: Option<u32>, per_page: Option<u32>) -> Self {
        Self {
            page: page.unwrap_or_else(default_page),
            per_page: per_page.unwrap_or_else(default_per_page),
        }
    }

    pub fn limit(&self) -> i64 {
        self.per_page.clamp(1, 100) as i64
    }
    pub fn offset(&self) -> i64 {
        (self.page.max(1) as i64 - 1) * self.limit()
    }
}
