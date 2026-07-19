//! rgit-core — domain layer: configuration, database, models, auth, permissions,
//! and on-disk storage paths (GitLab hashed-storage compatible).

pub mod auth;
pub mod config;
pub mod db;
pub mod error;
pub mod models;
pub mod path;
pub mod perm;
pub mod state;
pub mod storage;

pub use error::{Error, Result};
