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

/// GET /api/v1/groups — groups the caller belongs to (admin: all).
pub async fn list(
    State(state): State<AppState>,
    RequireUser(user, _): RequireUser,
) -> ApiResult<Json<Vec<Namespace>>> {
    let groups = if user.is_admin {
        sqlx::query_as::<_, Namespace>("SELECT * FROM namespaces WHERE kind = 'group' ORDER BY path")
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
    .bind(req.name.trim())
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
    if !user.is_admin && group_level(&state, id, user.id).await? < AccessLevel::Owner as i32 {
        return Err(Error::Forbidden.into());
    }
    if AccessLevel::from_i32(req.access_level).is_none() {
        return Err(Error::invalid("access_level must be 10/20/30/40/50").into());
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
    if member_id != user.id
        && !user.is_admin
        && group_level(&state, id, user.id).await? < AccessLevel::Owner as i32
    {
        return Err(Error::Forbidden.into());
    }
    let res = sqlx::query("DELETE FROM group_members WHERE namespace_id = ?1 AND user_id = ?2")
        .bind(id)
        .bind(member_id)
        .execute(&state.db)
        .await?;
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
