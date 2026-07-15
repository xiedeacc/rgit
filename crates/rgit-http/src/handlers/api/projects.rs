//! Project CRUD + archive/fork/transfer (DESIGN.md §10).

use axum::extract::{Path, Query, State};
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::Json;
use rgit_core::auth::{authorize_repo, effective_access_level};
use rgit_core::models::{Namespace, Project, User};
use rgit_core::perm::{AccessLevel, RepoAction, Visibility};
use rgit_core::state::AppState;
use rgit_core::{storage, Error};
use serde::Deserialize;

use super::Pagination;
use crate::error::ApiResult;
use crate::handlers::helpers::{find_project, repo_disk_path, validate_path_segment};
use crate::middleware::auth::{Identity, RequireUser};

/// Project id or URL-encoded "ns/path".
pub async fn locate(state: &AppState, id_or_path: &str) -> Result<Project, Error> {
    if let Ok(id) = id_or_path.parse::<i64>() {
        return sqlx::query_as::<_, Project>("SELECT * FROM projects WHERE id = ?1")
            .bind(id)
            .fetch_optional(&state.db)
            .await?
            .ok_or(Error::NotFound);
    }
    let (ns, path) = id_or_path.split_once('/').ok_or(Error::NotFound)?;
    Ok(find_project(state, ns, path).await?.1)
}

/// JSON representation with the namespace path expanded.
async fn render(state: &AppState, project: &Project) -> Result<serde_json::Value, Error> {
    let ns = sqlx::query_as::<_, Namespace>("SELECT * FROM namespaces WHERE id = ?1")
        .bind(project.namespace_id)
        .fetch_one(&state.db)
        .await?;
    let http_base = state.config.http.external_url.trim_end_matches('/');
    let ssh = &state.config.ssh;
    let full_path = format!("{}/{}", ns.path, project.path);
    let mut v = serde_json::to_value(project).expect("project serializes");
    let obj = v.as_object_mut().unwrap();
    obj.insert("full_path".into(), full_path.clone().into());
    obj.insert("namespace_path".into(), ns.path.clone().into());
    obj.insert("http_clone_url".into(), format!("{http_base}/{full_path}.git").into());
    obj.insert(
        "ssh_clone_url".into(),
        format!("ssh://git@{}:{}/{full_path}.git", ssh.clone_host, ssh.clone_port).into(),
    );
    Ok(v)
}

#[derive(Deserialize)]
pub struct ListQuery {
    #[serde(default)]
    pub search: Option<String>,
    #[serde(flatten)]
    pub page: Pagination,
}

/// GET /api/v1/projects — projects visible to the caller.
pub async fn list(
    State(state): State<AppState>,
    identity: Identity,
    Query(q): Query<ListQuery>,
) -> ApiResult<Response> {
    let (visible_floor, user_id, is_admin) = match &identity.user {
        Some(u) if u.is_admin => (Visibility::Private as i32, u.id, true),
        Some(u) => (Visibility::Internal as i32, u.id, false),
        None => (Visibility::Public as i32, -1, false),
    };
    let search = format!("%{}%", q.search.as_deref().unwrap_or("").replace('%', ""));

    // Visible = public/internal floor OR any membership/ownership path.
    let rows = sqlx::query_as::<_, Project>(
        r#"
        SELECT DISTINCT p.* FROM projects p
        LEFT JOIN project_members pm ON pm.project_id = p.id AND pm.user_id = ?2
        LEFT JOIN group_members gm ON gm.namespace_id = p.namespace_id AND gm.user_id = ?2
        LEFT JOIN namespaces n ON n.id = p.namespace_id
        WHERE (?4 OR p.visibility >= ?1 OR pm.user_id IS NOT NULL
               OR gm.user_id IS NOT NULL OR n.owner_user_id = ?2)
          AND (p.name LIKE ?3 OR p.path LIKE ?3 OR p.description LIKE ?3)
        ORDER BY p.updated_at DESC
        LIMIT ?5 OFFSET ?6
        "#,
    )
    .bind(visible_floor)
    .bind(user_id)
    .bind(&search)
    .bind(is_admin)
    .bind(q.page.limit())
    .bind(q.page.offset())
    .fetch_all(&state.db)
    .await?;

    let mut out = Vec::with_capacity(rows.len());
    for p in &rows {
        out.push(render(&state, p).await?);
    }
    Ok(Json(out).into_response())
}

#[derive(Deserialize)]
pub struct CreateProject {
    pub name: String,
    pub path: String,
    /// Defaults to the caller's user namespace.
    pub namespace_id: Option<i64>,
    #[serde(default)]
    pub visibility: i32,
    #[serde(default)]
    pub description: String,
}

/// POST /api/v1/projects
pub async fn create(
    State(state): State<AppState>,
    RequireUser(user, _): RequireUser,
    Json(req): Json<CreateProject>,
) -> ApiResult<Response> {
    validate_path_segment(&req.path)?;
    if Visibility::from_i32(req.visibility).is_none() {
        return Err(Error::invalid("visibility must be 0, 10 or 20").into());
    }

    let ns = match req.namespace_id {
        None => {
            sqlx::query_as::<_, Namespace>(
                "SELECT * FROM namespaces WHERE kind = 'user' AND owner_user_id = ?1",
            )
            .bind(user.id)
            .fetch_optional(&state.db)
            .await?
            .ok_or_else(|| Error::invalid("caller has no user namespace"))?
        }
        Some(id) => {
            let ns = sqlx::query_as::<_, Namespace>("SELECT * FROM namespaces WHERE id = ?1")
                .bind(id)
                .fetch_optional(&state.db)
                .await?
                .ok_or(Error::NotFound)?;
            // Must own the user namespace, or be >= Maintainer in the group.
            let allowed = match ns.kind.as_str() {
                "user" => ns.owner_user_id == Some(user.id),
                _ => {
                    let level: Option<i32> = sqlx::query_scalar(
                        "SELECT access_level FROM group_members WHERE namespace_id = ?1 AND user_id = ?2",
                    )
                    .bind(ns.id)
                    .bind(user.id)
                    .fetch_optional(&state.db)
                    .await?;
                    level.unwrap_or(0) >= AccessLevel::Maintainer as i32
                }
            };
            if !allowed && !user.is_admin {
                return Err(Error::Forbidden.into());
            }
            ns
        }
    };

    // disk_id: allocate from a monotonic sequence shared with migrated ids.
    let max_disk: i64 = sqlx::query_scalar("SELECT COALESCE(MAX(disk_id), 0) FROM projects")
        .fetch_one(&state.db)
        .await?;
    let disk_id = max_disk + 1;
    let disk_hash = storage::disk_hash(disk_id);

    let project = sqlx::query_as::<_, Project>(
        r#"
        INSERT INTO projects (namespace_id, path, name, description, visibility, disk_id, disk_hash)
        VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7) RETURNING *
        "#,
    )
    .bind(ns.id)
    .bind(&req.path)
    .bind(req.name.trim())
    .bind(&req.description)
    .bind(req.visibility)
    .bind(disk_id)
    .bind(&disk_hash)
    .fetch_one(&state.db)
    .await
    .map_err(|e| match e {
        sqlx::Error::Database(ref d) if d.is_unique_violation() => {
            Error::conflict("project path already taken in namespace")
        }
        other => Error::Db(other),
    })?;

    let repo = repo_disk_path(&state, &project);
    rgit_git::repo::init_bare(&state.config.git, &repo, "main").await?;

    Ok((StatusCode::CREATED, Json(render(&state, &project).await?)).into_response())
}

/// GET /api/v1/projects/{id|ns%2Fpath}
pub async fn get(
    State(state): State<AppState>,
    identity: Identity,
    Path(id): Path<String>,
) -> ApiResult<Response> {
    let project = locate(&state, &id).await?;
    authorize_repo(&state.db, identity.user.as_ref(), &project, RepoAction::Read).await?;
    Ok(Json(render(&state, &project).await?).into_response())
}

async fn require_admin_level(
    state: &AppState,
    user: &User,
    project: &Project,
    needed: AccessLevel,
) -> Result<(), Error> {
    if user.is_admin {
        return Ok(());
    }
    match effective_access_level(&state.db, user.id, project).await? {
        Some(level) if level >= needed => Ok(()),
        _ => Err(Error::Forbidden),
    }
}

#[derive(Deserialize)]
pub struct UpdateProject {
    pub name: Option<String>,
    pub description: Option<String>,
    pub visibility: Option<i32>,
    pub default_branch: Option<String>,
    pub lfs_enabled: Option<bool>,
}

/// PATCH /api/v1/projects/{id}
pub async fn update(
    State(state): State<AppState>,
    RequireUser(user, _): RequireUser,
    Path(id): Path<String>,
    Json(req): Json<UpdateProject>,
) -> ApiResult<Response> {
    let project = locate(&state, &id).await?;
    require_admin_level(&state, &user, &project, AccessLevel::Maintainer).await?;

    if let Some(v) = req.visibility {
        if Visibility::from_i32(v).is_none() {
            return Err(Error::invalid("visibility must be 0, 10 or 20").into());
        }
    }
    if let Some(branch) = &req.default_branch {
        let repo = repo_disk_path(&state, &project);
        rgit_git::read::rev_parse(&state.config.git, &repo, &format!("refs/heads/{branch}"))
            .await
            .map_err(|_| Error::invalid("branch does not exist"))?;
        rgit_git::repo::set_head_branch(&state.config.git, &repo, branch).await?;
    }

    let updated = sqlx::query_as::<_, Project>(
        r#"
        UPDATE projects SET
            name = COALESCE(?1, name),
            description = COALESCE(?2, description),
            visibility = COALESCE(?3, visibility),
            default_branch = COALESCE(?4, default_branch),
            lfs_enabled = COALESCE(?5, lfs_enabled),
            updated_at = datetime('now')
        WHERE id = ?6 RETURNING *
        "#,
    )
    .bind(&req.name)
    .bind(&req.description)
    .bind(req.visibility)
    .bind(&req.default_branch)
    .bind(req.lfs_enabled)
    .bind(project.id)
    .fetch_one(&state.db)
    .await?;

    Ok(Json(render(&state, &updated).await?).into_response())
}

/// DELETE /api/v1/projects/{id} — Owner/admin only. Repo dir is soft-deleted.
pub async fn delete(
    State(state): State<AppState>,
    RequireUser(user, _): RequireUser,
    Path(id): Path<String>,
) -> ApiResult<StatusCode> {
    let project = locate(&state, &id).await?;
    require_admin_level(&state, &user, &project, AccessLevel::Owner).await?;

    sqlx::query("DELETE FROM projects WHERE id = ?1")
        .bind(project.id)
        .execute(&state.db)
        .await?;
    let repo = repo_disk_path(&state, &project);
    if repo.exists() {
        rgit_git::repo::soft_delete(&repo)?;
    }
    Ok(StatusCode::NO_CONTENT)
}

/// POST /api/v1/projects/{id}/archive | unarchive
pub async fn set_archived(
    State(state): State<AppState>,
    RequireUser(user, _): RequireUser,
    Path((id, flag)): Path<(String, String)>,
) -> ApiResult<Response> {
    let archived = match flag.as_str() {
        "archive" => true,
        "unarchive" => false,
        _ => return Err(Error::NotFound.into()),
    };
    let project = locate(&state, &id).await?;
    require_admin_level(&state, &user, &project, AccessLevel::Maintainer).await?;

    let updated = sqlx::query_as::<_, Project>(
        "UPDATE projects SET archived = ?1, updated_at = datetime('now') WHERE id = ?2 RETURNING *",
    )
    .bind(archived)
    .bind(project.id)
    .fetch_one(&state.db)
    .await?;
    Ok(Json(render(&state, &updated).await?).into_response())
}

#[derive(Deserialize)]
pub struct ForkRequest {
    pub namespace_id: Option<i64>,
    pub path: Option<String>,
    pub name: Option<String>,
}

/// POST /api/v1/projects/{id}/fork — bare local clone + LFS link copy.
pub async fn fork(
    State(state): State<AppState>,
    RequireUser(user, _): RequireUser,
    Path(id): Path<String>,
    Json(req): Json<ForkRequest>,
) -> ApiResult<Response> {
    let source = locate(&state, &id).await?;
    authorize_repo(&state.db, Some(&user), &source, RepoAction::Read).await?;

    let target_ns_id = match req.namespace_id {
        Some(id) => id,
        None => sqlx::query_scalar(
            "SELECT id FROM namespaces WHERE kind = 'user' AND owner_user_id = ?1",
        )
        .bind(user.id)
        .fetch_one(&state.db)
        .await?,
    };
    let path = req.path.unwrap_or_else(|| source.path.clone());
    validate_path_segment(&path)?;

    let max_disk: i64 = sqlx::query_scalar("SELECT COALESCE(MAX(disk_id), 0) FROM projects")
        .fetch_one(&state.db)
        .await?;
    let disk_id = max_disk + 1;
    let disk_hash = storage::disk_hash(disk_id);

    let forked = sqlx::query_as::<_, Project>(
        r#"
        INSERT INTO projects (namespace_id, path, name, description, visibility,
                              default_branch, lfs_enabled, disk_id, disk_hash,
                              forked_from_project_id)
        VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10) RETURNING *
        "#,
    )
    .bind(target_ns_id)
    .bind(&path)
    .bind(req.name.unwrap_or_else(|| source.name.clone()))
    .bind(&source.description)
    .bind(source.visibility)
    .bind(&source.default_branch)
    .bind(source.lfs_enabled)
    .bind(disk_id)
    .bind(&disk_hash)
    .bind(source.id)
    .fetch_one(&state.db)
    .await
    .map_err(|e| match e {
        sqlx::Error::Database(ref d) if d.is_unique_violation() => {
            Error::conflict("target path already taken")
        }
        other => Error::Db(other),
    })?;

    let src_repo = repo_disk_path(&state, &source);
    let dst_repo = repo_disk_path(&state, &forked);
    rgit_git::repo::fork_local(&state.config.git, &src_repo, &dst_repo).await?;

    // Share LFS objects with the fork (DESIGN.md §9).
    sqlx::query(
        r#"
        INSERT OR IGNORE INTO project_lfs_objects (project_id, lfs_object_id)
        SELECT ?1, lfs_object_id FROM project_lfs_objects WHERE project_id = ?2
        "#,
    )
    .bind(forked.id)
    .bind(source.id)
    .execute(&state.db)
    .await?;

    Ok((StatusCode::CREATED, Json(render(&state, &forked).await?)).into_response())
}

#[derive(Deserialize)]
pub struct TransferRequest {
    pub namespace_id: i64,
}

/// POST /api/v1/projects/{id}/transfer — move to another namespace (Owner).
pub async fn transfer(
    State(state): State<AppState>,
    RequireUser(user, _): RequireUser,
    Path(id): Path<String>,
    Json(req): Json<TransferRequest>,
) -> ApiResult<Response> {
    let project = locate(&state, &id).await?;
    require_admin_level(&state, &user, &project, AccessLevel::Owner).await?;

    let updated = sqlx::query_as::<_, Project>(
        "UPDATE projects SET namespace_id = ?1, updated_at = datetime('now') WHERE id = ?2 RETURNING *",
    )
    .bind(req.namespace_id)
    .bind(project.id)
    .fetch_one(&state.db)
    .await
    .map_err(|e| match e {
        sqlx::Error::Database(ref d) if d.is_unique_violation() => {
            Error::conflict("path already taken in target namespace")
        }
        other => Error::Db(other),
    })?;
    // Disk path is hash-based, so transfers never move data. (GitLab parity.)
    Ok(Json(render(&state, &updated).await?).into_response())
}
