//! Shared handler helpers: project resolution and identifier validation.

use rgit_core::models::{Namespace, Project};
use rgit_core::state::AppState;
use rgit_core::{Error, Result};

/// Path-segment policy (DESIGN.md §7.5): alnum start, then [A-Za-z0-9_.-];
/// no ".."/"."; no reserved suffixes; not starting with '@' (reserved for
/// storage prefixes like @hashed).
pub fn validate_path_segment(s: &str) -> Result<()> {
    let ok_chars = s
        .bytes()
        .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'_' | b'.' | b'-'));
    let ok = !s.is_empty()
        && s.len() <= 255
        && s.as_bytes()[0].is_ascii_alphanumeric()
        && ok_chars
        && s != "."
        && s != ".."
        && !s.ends_with(".git")
        && !s.ends_with(".wiki")
        && !s.ends_with(".atom");
    if ok {
        Ok(())
    } else {
        Err(Error::invalid("invalid path segment"))
    }
}

/// Reserved first path segments that can never be namespaces.
pub const RESERVED_ROOTS: &[&str] = &[
    "api", "admin", "login", "settings", "groups", "assets", "-", "explore",
];

pub fn is_reserved_root(s: &str) -> bool {
    RESERVED_ROOTS.contains(&s)
}

/// Look up a project by namespace path + project path (case-insensitive).
pub async fn find_project(
    state: &AppState,
    ns_path: &str,
    project_path: &str,
) -> Result<(Namespace, Project)> {
    let ns = sqlx::query_as::<_, Namespace>("SELECT * FROM namespaces WHERE path = ?1")
        .bind(ns_path)
        .fetch_optional(&state.db)
        .await?
        .ok_or(Error::NotFound)?;
    let project = sqlx::query_as::<_, Project>(
        "SELECT * FROM projects WHERE namespace_id = ?1 AND path = ?2",
    )
    .bind(ns.id)
    .bind(project_path)
    .fetch_optional(&state.db)
    .await?
    .ok_or(Error::NotFound)?;
    Ok((ns, project))
}

/// Absolute bare-repo path for a project.
pub fn repo_disk_path(state: &AppState, project: &Project) -> std::path::PathBuf {
    rgit_core::storage::repo_path(&state.config.storage, &project.disk_hash)
}
