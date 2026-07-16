//! Groups (flat namespaces of kind 'group') + group members.

use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::Json;
use rgit_core::models::Namespace;
use rgit_core::perm::AccessLevel;
use rgit_core::state::AppState;
use rgit_core::Error;
use serde::Deserialize;

use crate::error::ApiResult;
use crate::handlers::helpers::{is_reserved_root, validate_path_segment};
use crate::middleware::auth::RequireUser;

fn map_group_owner_invariant(error: sqlx::Error) -> Error {
    if error
        .to_string()
        .contains("group must retain at least one owner")
    {
        Error::conflict("group must retain at least one owner")
    } else {
        Error::Db(error)
    }
}

async fn group_level(state: &AppState, ns_id: i64, user_id: i64) -> Result<i32, Error> {
    let level: Option<i32> = sqlx::query_scalar(
        "SELECT access_level FROM group_members WHERE namespace_id = ?1 AND user_id = ?2",
    )
    .bind(ns_id)
    .bind(user_id)
    .fetch_optional(&state.db)
    .await?;
    Ok(level.unwrap_or(0))
}

async fn locate_group(state: &AppState, id_or_path: &str) -> Result<Namespace, Error> {
    let group = if let Ok(id) = id_or_path.parse::<i64>() {
        sqlx::query_as::<_, Namespace>("SELECT * FROM namespaces WHERE id = ?1 AND kind = 'group'")
            .bind(id)
            .fetch_optional(&state.db)
            .await?
    } else {
        sqlx::query_as::<_, Namespace>(
            "SELECT * FROM namespaces WHERE path = ?1 AND kind = 'group'",
        )
        .bind(id_or_path)
        .fetch_optional(&state.db)
        .await?
    };
    group.ok_or(Error::NotFound)
}

fn validate_name(name: &str) -> Result<&str, Error> {
    let name = name.trim();
    if name.is_empty() || name.len() > 255 {
        Err(Error::invalid("name must be between 1 and 255 bytes"))
    } else {
        Ok(name)
    }
}

/// GET /api/v1/groups — groups the caller belongs to (admin: all).
pub async fn list(
    State(state): State<AppState>,
    RequireUser(user, _): RequireUser,
) -> ApiResult<Json<Vec<Namespace>>> {
    let groups = if user.is_admin {
        sqlx::query_as::<_, Namespace>(
            "SELECT * FROM namespaces WHERE kind = 'group' ORDER BY path",
        )
        .fetch_all(&state.db)
        .await?
    } else {
        sqlx::query_as::<_, Namespace>(
            r#"
            SELECT n.* FROM namespaces n
            JOIN group_members gm ON gm.namespace_id = n.id
            WHERE n.kind = 'group' AND gm.user_id = ?1 ORDER BY n.path
            "#,
        )
        .bind(user.id)
        .fetch_all(&state.db)
        .await?
    };
    Ok(Json(groups))
}

/// GET /api/v1/groups/{id-or-path} — group members and admins only.
pub async fn get(
    State(state): State<AppState>,
    RequireUser(user, _): RequireUser,
    Path(id): Path<String>,
) -> ApiResult<Json<Namespace>> {
    let group = locate_group(&state, &id).await?;
    if !user.is_admin && group_level(&state, group.id, user.id).await? == 0 {
        return Err(Error::NotFound.into());
    }
    Ok(Json(group))
}

#[derive(Deserialize)]
pub struct CreateGroup {
    pub name: String,
    pub path: String,
    #[serde(default)]
    pub description: String,
}

/// POST /api/v1/groups — creator becomes Owner.
pub async fn create(
    State(state): State<AppState>,
    RequireUser(user, _): RequireUser,
    Json(req): Json<CreateGroup>,
) -> ApiResult<Response> {
    validate_path_segment(&req.path)?;
    let name = validate_name(&req.name)?;
    if is_reserved_root(&req.path) {
        return Err(Error::invalid("path is reserved").into());
    }

    let mut tx = state.db.begin().await?;
    let ns = sqlx::query_as::<_, Namespace>(
        r#"
        INSERT INTO namespaces (path, name, kind, description)
        VALUES (?1, ?2, 'group', ?3) RETURNING *
        "#,
    )
    .bind(&req.path)
    .bind(name)
    .bind(&req.description)
    .fetch_one(&mut *tx)
    .await
    .map_err(|e| match e {
        sqlx::Error::Database(ref d) if d.is_unique_violation() => {
            Error::conflict("path already taken")
        }
        other => Error::Db(other),
    })?;
    sqlx::query(
        "INSERT INTO group_members (namespace_id, user_id, access_level) VALUES (?1, ?2, ?3)",
    )
    .bind(ns.id)
    .bind(user.id)
    .bind(AccessLevel::Owner as i32)
    .execute(&mut *tx)
    .await?;
    tx.commit().await?;

    Ok((StatusCode::CREATED, Json(ns)).into_response())
}

#[derive(Deserialize)]
pub struct UpdateGroup {
    pub name: Option<String>,
    pub path: Option<String>,
    pub description: Option<String>,
}

/// PATCH /api/v1/groups/{id-or-path} — Owner/admin required.
pub async fn update(
    State(state): State<AppState>,
    RequireUser(user, _): RequireUser,
    Path(id): Path<String>,
    Json(req): Json<UpdateGroup>,
) -> ApiResult<Json<Namespace>> {
    let group = locate_group(&state, &id).await?;
    if !user.is_admin && group_level(&state, group.id, user.id).await? < AccessLevel::Owner as i32 {
        return Err(Error::Forbidden.into());
    }
    let name = req.name.as_deref().map(validate_name).transpose()?;
    if let Some(path) = req.path.as_deref() {
        validate_path_segment(path)?;
        if is_reserved_root(path) {
            return Err(Error::invalid("path is reserved").into());
        }
    }
    let updated = sqlx::query_as::<_, Namespace>(
        r#"
        UPDATE namespaces SET
            name = COALESCE(?1, name),
            path = COALESCE(?2, path),
            description = COALESCE(?3, description),
            updated_at = datetime('now')
        WHERE id = ?4 AND kind = 'group' RETURNING *
        "#,
    )
    .bind(name)
    .bind(&req.path)
    .bind(&req.description)
    .bind(group.id)
    .fetch_one(&state.db)
    .await
    .map_err(|error| match error {
        sqlx::Error::Database(ref database) if database.is_unique_violation() => {
            Error::conflict("path already taken")
        }
        other => Error::Db(other),
    })?;
    Ok(Json(updated))
}

/// GET /api/v1/groups/{id}/members — any member/admin may inspect membership.
pub async fn list_members(
    State(state): State<AppState>,
    RequireUser(user, _): RequireUser,
    Path(id): Path<i64>,
) -> ApiResult<Response> {
    locate_group(&state, &id.to_string()).await?;
    if !user.is_admin && group_level(&state, id, user.id).await? == 0 {
        return Err(Error::NotFound.into());
    }
    let rows: Vec<(i64, String, String, i32, String)> = sqlx::query_as(
        r#"
        SELECT u.id, u.username, u.name, gm.access_level, gm.created_at
        FROM group_members gm JOIN users u ON u.id = gm.user_id
        WHERE gm.namespace_id = ?1
        ORDER BY gm.access_level DESC, u.username
        "#,
    )
    .bind(id)
    .fetch_all(&state.db)
    .await?;
    let members = rows
        .into_iter()
        .map(|(user_id, username, name, access_level, created_at)| {
            serde_json::json!({
                "user_id": user_id,
                "username": username,
                "name": name,
                "access_level": access_level,
                "created_at": created_at,
            })
        })
        .collect::<Vec<_>>();
    Ok(Json(members).into_response())
}

#[derive(Deserialize)]
pub struct AddGroupMember {
    pub user_id: i64,
    pub access_level: i32,
}

/// POST /api/v1/groups/{id}/members — Owner required.
pub async fn add_member(
    State(state): State<AppState>,
    RequireUser(user, _): RequireUser,
    Path(id): Path<i64>,
    Json(req): Json<AddGroupMember>,
) -> ApiResult<Response> {
    locate_group(&state, &id.to_string()).await?;
    if !user.is_admin && group_level(&state, id, user.id).await? < AccessLevel::Owner as i32 {
        return Err(Error::Forbidden.into());
    }
    if AccessLevel::from_i32(req.access_level).is_none() {
        return Err(Error::invalid("access_level must be 10/20/30/40/50").into());
    }
    let existing = group_level(&state, id, req.user_id).await?;
    if existing == AccessLevel::Owner as i32 && req.access_level != AccessLevel::Owner as i32 {
        let owners: i64 = sqlx::query_scalar(
            "SELECT COUNT(*) FROM group_members WHERE namespace_id = ?1 AND access_level = ?2",
        )
        .bind(id)
        .bind(AccessLevel::Owner as i32)
        .fetch_one(&state.db)
        .await?;
        if owners <= 1 {
            return Err(Error::conflict("group must retain at least one owner").into());
        }
    }
    sqlx::query(
        r#"
        INSERT INTO group_members (namespace_id, user_id, access_level) VALUES (?1, ?2, ?3)
        ON CONFLICT (namespace_id, user_id) DO UPDATE SET access_level = excluded.access_level
        "#,
    )
    .bind(id)
    .bind(req.user_id)
    .bind(req.access_level)
    .execute(&state.db)
    .await
    .map_err(|e| match e {
        e if e
            .to_string()
            .contains("group must retain at least one owner") =>
        {
            Error::conflict("group must retain at least one owner")
        }
        sqlx::Error::Database(ref d) if d.is_foreign_key_violation() => {
            Error::invalid("group or user does not exist")
        }
        other => Error::Db(other),
    })?;
    Ok(StatusCode::CREATED.into_response())
}

/// DELETE /api/v1/groups/{id}/members/{user_id}
pub async fn remove_member(
    State(state): State<AppState>,
    RequireUser(user, _): RequireUser,
    Path((id, member_id)): Path<(i64, i64)>,
) -> ApiResult<StatusCode> {
    locate_group(&state, &id.to_string()).await?;
    if member_id != user.id
        && !user.is_admin
        && group_level(&state, id, user.id).await? < AccessLevel::Owner as i32
    {
        return Err(Error::Forbidden.into());
    }
    let target_level = group_level(&state, id, member_id).await?;
    if target_level == AccessLevel::Owner as i32 {
        let owners: i64 = sqlx::query_scalar(
            "SELECT COUNT(*) FROM group_members WHERE namespace_id = ?1 AND access_level = ?2",
        )
        .bind(id)
        .bind(AccessLevel::Owner as i32)
        .fetch_one(&state.db)
        .await?;
        if owners <= 1 {
            return Err(Error::conflict("group must retain at least one owner").into());
        }
    }
    let res = sqlx::query("DELETE FROM group_members WHERE namespace_id = ?1 AND user_id = ?2")
        .bind(id)
        .bind(member_id)
        .execute(&state.db)
        .await
        .map_err(map_group_owner_invariant)?;
    if res.rows_affected() == 0 {
        return Err(Error::NotFound.into());
    }
    Ok(StatusCode::NO_CONTENT)
}

/// DELETE /api/v1/groups/{id} — only when empty of projects (Owner/admin).
pub async fn delete(
    State(state): State<AppState>,
    RequireUser(user, _): RequireUser,
    Path(id): Path<i64>,
) -> ApiResult<StatusCode> {
    if !user.is_admin && group_level(&state, id, user.id).await? < AccessLevel::Owner as i32 {
        return Err(Error::Forbidden.into());
    }
    let projects: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM projects WHERE namespace_id = ?1")
        .bind(id)
        .fetch_one(&state.db)
        .await?;
    if projects > 0 {
        return Err(Error::conflict("group still contains projects").into());
    }
    let res = sqlx::query("DELETE FROM namespaces WHERE id = ?1 AND kind = 'group'")
        .bind(id)
        .execute(&state.db)
        .await?;
    if res.rows_affected() == 0 {
        return Err(Error::NotFound.into());
    }
    Ok(StatusCode::NO_CONTENT)
}
