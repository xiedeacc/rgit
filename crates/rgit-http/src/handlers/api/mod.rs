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

use serde::Deserialize;

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
    pub fn limit(&self) -> i64 {
        self.per_page.clamp(1, 100) as i64
    }
    pub fn offset(&self) -> i64 {
        (self.page.max(1) as i64 - 1) * self.limit()
    }
}
