//! Project member management: /api/v1/projects/{id}/members

use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::Json;
use rgit_core::auth::effective_access_level;
use rgit_core::perm::AccessLevel;
use rgit_core::state::AppState;
use rgit_core::Error;
use serde::Deserialize;

use super::projects::locate;
use crate::error::ApiResult;
use crate::middleware::auth::RequireUser;

async fn require_maintainer(
    state: &AppState,
    user: &rgit_core::models::User,
    project: &rgit_core::models::Project,
) -> Result<(), Error> {
    if user.is_admin {
        return Ok(());
    }
    match effective_access_level(&state.db, user.id, project).await? {
        Some(level) if level >= AccessLevel::Maintainer => Ok(()),
        _ => Err(Error::Forbidden),
    }
}

/// GET .../members — members with usernames (project-level only).
pub async fn list(
    State(state): State<AppState>,
    RequireUser(user, _): RequireUser,
    Path(id): Path<String>,
) -> ApiResult<Response> {
    let project = locate(&state, &id).await?;
    rgit_core::auth::authorize_repo(
        &state.db,
        Some(&user),
        &project,
        rgit_core::perm::RepoAction::Read,
    )
    .await?;

    let rows: Vec<(i64, String, String, i32, String)> = sqlx::query_as(
        r#"
        SELECT u.id, u.username, u.name, pm.access_level, pm.created_at
        FROM project_members pm JOIN users u ON u.id = pm.user_id
        WHERE pm.project_id = ?1 ORDER BY pm.access_level DESC, u.username
        "#,
    )
    .bind(project.id)
    .fetch_all(&state.db)
    .await?;

    let members: Vec<_> = rows
        .into_iter()
        .map(|(user_id, username, name, access_level, created_at)| {
            serde_json::json!({
                "user_id": user_id, "username": username, "name": name,
                "access_level": access_level, "created_at": created_at,
            })
        })
        .collect();
    Ok(Json(members).into_response())
}

#[derive(Deserialize)]
pub struct AddMember {
    pub user_id: i64,
    pub access_level: i32,
}

/// POST .../members
pub async fn add(
    State(state): State<AppState>,
    RequireUser(user, _): RequireUser,
    Path(id): Path<String>,
    Json(req): Json<AddMember>,
) -> ApiResult<Response> {
    let project = locate(&state, &id).await?;
    require_maintainer(&state, &user, &project).await?;
    if AccessLevel::from_i32(req.access_level).is_none() {
        return Err(Error::invalid("access_level must be 10/20/30/40/50").into());
    }

    sqlx::query(
        r#"
        INSERT INTO project_members (project_id, user_id, access_level) VALUES (?1, ?2, ?3)
        ON CONFLICT (project_id, user_id) DO UPDATE SET access_level = excluded.access_level
        "#,
    )
    .bind(project.id)
    .bind(req.user_id)
    .bind(req.access_level)
    .execute(&state.db)
    .await
    .map_err(|e| match e {
        sqlx::Error::Database(ref d) if d.is_foreign_key_violation() => {
            Error::invalid("user does not exist")
        }
        other => Error::Db(other),
    })?;
    Ok(StatusCode::CREATED.into_response())
}

/// DELETE .../members/{user_id}
pub async fn remove(
    State(state): State<AppState>,
    RequireUser(user, _): RequireUser,
    Path((id, member_id)): Path<(String, i64)>,
) -> ApiResult<StatusCode> {
    let project = locate(&state, &id).await?;
    // Members may remove themselves; otherwise Maintainer required.
    if member_id != user.id {
        require_maintainer(&state, &user, &project).await?;
    }
    let res = sqlx::query("DELETE FROM project_members WHERE project_id = ?1 AND user_id = ?2")
        .bind(project.id)
        .bind(member_id)
        .execute(&state.db)
        .await?;
    if res.rows_affected() == 0 {
        return Err(Error::NotFound.into());
    }
    Ok(StatusCode::NO_CONTENT)
}
