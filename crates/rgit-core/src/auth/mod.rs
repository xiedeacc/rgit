//! Authentication: passwords (bcrypt, GitLab-compatible), sessions,
//! personal access tokens, SSH key parsing/fingerprints, and the
//! project-permission resolver.

pub mod authorized_keys;
pub mod lfs_token;
pub mod password;
pub mod session;
pub mod sshkey;
pub mod token;

use crate::models::{Project, User};
use crate::perm::{required_level, AccessLevel, RepoAction, Visibility};
use crate::{Error, Result};
use sqlx::SqlitePool;

/// Effective access level of a user on a project: max(project membership,
/// group membership, namespace ownership).
pub async fn effective_access_level(
    db: &SqlitePool,
    user_id: i64,
    project: &Project,
) -> Result<Option<AccessLevel>> {
    let level: Option<i32> = sqlx::query_scalar(
        r#"
        SELECT MAX(level) FROM (
            SELECT access_level AS level FROM project_members
             WHERE project_id = ?1 AND user_id = ?2
            UNION ALL
            SELECT access_level FROM group_members
             WHERE namespace_id = ?3 AND user_id = ?2
            UNION ALL
            SELECT 50 FROM namespaces
             WHERE id = ?3 AND owner_user_id = ?2
        )
        "#,
    )
    .bind(project.id)
    .bind(user_id)
    .bind(project.namespace_id)
    .fetch_one(db)
    .await?;

    Ok(level.and_then(AccessLevel::from_i32))
}

/// Authorize `action` on `project` for an optional authenticated user.
/// Admins bypass membership checks. Archived projects reject writes.
pub async fn authorize_repo(
    db: &SqlitePool,
    user: Option<&User>,
    project: &Project,
    action: RepoAction,
) -> Result<()> {
    if action == RepoAction::Write && project.archived {
        return Err(Error::Forbidden);
    }

    let visibility = Visibility::from_i32(project.visibility).unwrap_or(Visibility::Private);

    match user {
        None => {
            // Anonymous: read on public projects only.
            if action == RepoAction::Read && visibility == Visibility::Public {
                Ok(())
            } else {
                Err(Error::Unauthorized)
            }
        }
        Some(user) => {
            if !user.is_active() {
                return Err(Error::Forbidden);
            }
            if user.is_admin {
                return Ok(());
            }
            // Any signed-in active user can read public/internal projects.
            if action == RepoAction::Read && visibility >= Visibility::Internal {
                return Ok(());
            }
            match effective_access_level(db, user.id, project).await? {
                Some(level) if level >= required_level(action) => Ok(()),
                _ => Err(Error::Forbidden),
            }
        }
    }
}
