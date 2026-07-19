//! Shared handler helpers: project resolution and identifier validation.

use rgit_core::models::{Namespace, Project};
use rgit_core::state::AppState;
use rgit_core::{Error, Result};

pub fn validate_path_segment(s: &str) -> Result<()> {
    rgit_core::path::validate_path_segment(s)
}

pub fn is_reserved_root(s: &str) -> bool {
    rgit_core::path::is_reserved_root(s)
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
