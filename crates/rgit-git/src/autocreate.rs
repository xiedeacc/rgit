//! Auto-create group + project when an authenticated push targets a missing repo.

use rgit_core::config::{GitConfig, StorageConfig};
use rgit_core::models::{Namespace, Project, User};
use rgit_core::perm::AccessLevel;
use rgit_core::{path, storage, Error, Result};
use sqlx::{Sqlite, SqlitePool, Transaction};

use crate::repo;

pub async fn ensure_project_for_push(
    db: &SqlitePool,
    git: &GitConfig,
    storage_cfg: &StorageConfig,
    user: &User,
    namespace_path: &str,
    project_path: &str,
) -> Result<Project> {
    path::validate_path_segment(namespace_path)?;
    path::validate_path_segment(project_path)?;
    if path::is_reserved_root(namespace_path) {
        return Err(Error::invalid("namespace path is reserved"));
    }

    let namespace = ensure_writable_namespace(db, user, namespace_path).await?;
    if let Some(project) = find_project(db, namespace.id, project_path).await? {
        return Ok(project);
    }

    let disk_id = reserve_disk_id(db).await?;
    let disk_hash = storage::disk_hash(disk_id);
    let repo_path = storage::repo_path(storage_cfg, &disk_hash);
    if let Err(error) = repo::init_bare(git, &repo_path, "main").await {
        cleanup_new_repo(&repo_path);
        return Err(error);
    }

    let inserted = sqlx::query_as::<_, Project>(
        r#"
        INSERT INTO projects (namespace_id, path, name, description, visibility, disk_id, disk_hash)
        VALUES (?1, ?2, ?3, '', 0, ?4, ?5)
        RETURNING *
        "#,
    )
    .bind(namespace.id)
    .bind(project_path)
    .bind(project_path)
    .bind(disk_id)
    .bind(&disk_hash)
    .fetch_one(db)
    .await;

    match inserted {
        Ok(project) => Ok(project),
        Err(sqlx::Error::Database(error)) if error.is_unique_violation() => {
            cleanup_new_repo(&repo_path);
            find_project(db, namespace.id, project_path)
                .await?
                .ok_or(Error::NotFound)
        }
        Err(error) => {
            cleanup_new_repo(&repo_path);
            Err(Error::Db(error))
        }
    }
}

async fn ensure_writable_namespace(
    db: &SqlitePool,
    user: &User,
    namespace_path: &str,
) -> Result<Namespace> {
    if let Some(namespace) =
        sqlx::query_as::<_, Namespace>("SELECT * FROM namespaces WHERE path = ?1")
            .bind(namespace_path)
            .fetch_optional(db)
            .await?
    {
        if user.is_admin || namespace.owner_user_id == Some(user.id) {
            return Ok(namespace);
        }
        if namespace.kind == "group" {
            let level: Option<i32> = sqlx::query_scalar(
                "SELECT access_level FROM group_members WHERE namespace_id = ?1 AND user_id = ?2",
            )
            .bind(namespace.id)
            .bind(user.id)
            .fetch_optional(db)
            .await?;
            if level.unwrap_or(0) >= AccessLevel::Maintainer as i32 {
                return Ok(namespace);
            }
        }
        return Err(Error::Forbidden);
    }

    let mut tx = db.begin().await?;
    let namespace = sqlx::query_as::<_, Namespace>(
        r#"
        INSERT INTO namespaces (path, name, kind, owner_user_id)
        VALUES (?1, ?1, 'group', ?2)
        RETURNING *
        "#,
    )
    .bind(namespace_path)
    .bind(user.id)
    .fetch_one(&mut *tx)
    .await
    .map_err(|error| map_namespace_insert_error(error, namespace_path))?;
    sqlx::query(
        r#"
        INSERT INTO group_members (namespace_id, user_id, access_level)
        VALUES (?1, ?2, ?3)
        ON CONFLICT(namespace_id, user_id) DO UPDATE SET access_level = excluded.access_level
        "#,
    )
    .bind(namespace.id)
    .bind(user.id)
    .bind(AccessLevel::Owner as i32)
    .execute(&mut *tx)
    .await?;
    tx.commit().await?;
    Ok(namespace)
}

fn map_namespace_insert_error(error: sqlx::Error, namespace_path: &str) -> Error {
    match error {
        sqlx::Error::Database(db_error) if db_error.is_unique_violation() => {
            Error::conflict(format!("namespace already exists: {namespace_path}"))
        }
        other => Error::Db(other),
    }
}

async fn reserve_disk_id(db: &SqlitePool) -> Result<i64> {
    let mut tx = db.begin().await?;
    let id = allocate_disk_id(&mut tx).await?;
    tx.commit().await?;
    Ok(id)
}

async fn allocate_disk_id(tx: &mut Transaction<'_, Sqlite>) -> Result<i64> {
    Ok(sqlx::query_scalar(
        r#"
        INSERT INTO project_disk_id_allocations (id)
        SELECT COALESCE(MAX(id), 0) + 1 FROM (
            SELECT disk_id AS id FROM projects
            UNION ALL
            SELECT id FROM project_disk_id_allocations
        )
        RETURNING id
        "#,
    )
    .fetch_one(&mut **tx)
    .await?)
}

async fn find_project(
    db: &SqlitePool,
    namespace_id: i64,
    project_path: &str,
) -> Result<Option<Project>> {
    Ok(
        sqlx::query_as::<_, Project>(
            "SELECT * FROM projects WHERE namespace_id = ?1 AND path = ?2",
        )
        .bind(namespace_id)
        .bind(project_path)
        .fetch_optional(db)
        .await?,
    )
}

fn cleanup_new_repo(path: &std::path::Path) {
    if let Err(error) = std::fs::remove_dir_all(path) {
        if error.kind() != std::io::ErrorKind::NotFound {
            tracing::error!(%error, path = %path.display(), "failed to clean up auto-created repo");
        }
    }
}
