//! Admin panel API (is_admin only): user management + instance stats.

use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::Json;
use rgit_core::auth::password;
use rgit_core::models::{Project, User};
use rgit_core::state::AppState;
use rgit_core::Error;
use serde::Deserialize;

use super::{paginated_json, Pagination};
use crate::error::ApiResult;
use crate::middleware::auth::RequireAdmin;

fn map_admin_invariant(error: sqlx::Error) -> Error {
    if error
        .to_string()
        .contains("instance must retain at least one active admin")
    {
        Error::conflict("instance must retain at least one active admin")
    } else {
        Error::Db(error)
    }
}

/// GET /api/v1/admin/users
pub async fn list_users(
    State(state): State<AppState>,
    RequireAdmin(_): RequireAdmin,
    axum::extract::Query(page): axum::extract::Query<Pagination>,
) -> ApiResult<Response> {
    let total: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM users")
        .fetch_one(&state.db)
        .await?;
    let users = sqlx::query_as::<_, User>("SELECT * FROM users ORDER BY id LIMIT ?1 OFFSET ?2")
        .bind(page.limit())
        .bind(page.offset())
        .fetch_all(&state.db)
        .await?;
    Ok(paginated_json(users, total))
}

#[derive(Deserialize)]
pub struct CreateUser {
    pub username: String,
    pub email: String,
    pub name: String,
    pub password: String,
    #[serde(default)]
    pub is_admin: bool,
}

/// POST /api/v1/admin/users — also creates the user namespace.
pub async fn create_user(
    State(state): State<AppState>,
    RequireAdmin(_): RequireAdmin,
    Json(req): Json<CreateUser>,
) -> ApiResult<Response> {
    crate::handlers::helpers::validate_path_segment(&req.username)?;
    if crate::handlers::helpers::is_reserved_root(&req.username) {
        return Err(Error::invalid("username is reserved").into());
    }
    if !req.email.contains('@') {
        return Err(Error::invalid("invalid email").into());
    }
    password::check_password_policy(&req.password, state.config.auth.min_password_length)?;
    let hash =
        password::hash_password_async(req.password.clone(), state.config.auth.bcrypt_cost).await?;

    let mut tx = state.db.begin().await?;
    let user = sqlx::query_as::<_, User>(
        r#"
        INSERT INTO users (username, email, name, password_hash, is_admin)
        VALUES (?1, ?2, ?3, ?4, ?5) RETURNING *
        "#,
    )
    .bind(&req.username)
    .bind(&req.email)
    .bind(req.name.trim())
    .bind(&hash)
    .bind(req.is_admin)
    .fetch_one(&mut *tx)
    .await
    .map_err(|e| match e {
        sqlx::Error::Database(ref d) if d.is_unique_violation() => {
            Error::conflict("username or email already taken")
        }
        other => Error::Db(other),
    })?;
    sqlx::query(
        "INSERT INTO namespaces (path, name, kind, owner_user_id) VALUES (?1, ?2, 'user', ?3)",
    )
    .bind(&user.username)
    .bind(&user.name)
    .bind(user.id)
    .execute(&mut *tx)
    .await?;
    tx.commit().await?;

    Ok((StatusCode::CREATED, Json(user)).into_response())
}

#[derive(Deserialize)]
pub struct UpdateUser {
    pub name: Option<String>,
    pub email: Option<String>,
    pub is_admin: Option<bool>,
    /// "active" | "blocked"
    pub state: Option<String>,
    /// Admin password reset (no old password needed).
    pub password: Option<String>,
}

/// PATCH /api/v1/admin/users/{id}
pub async fn update_user(
    State(state): State<AppState>,
    RequireAdmin(admin): RequireAdmin,
    Path(id): Path<i64>,
    Json(req): Json<UpdateUser>,
) -> ApiResult<Json<User>> {
    if let Some(email) = &req.email {
        if !email.contains('@') {
            return Err(Error::invalid("invalid email").into());
        }
    }
    if let Some(s) = &req.state {
        if !matches!(s.as_str(), "active" | "blocked") {
            return Err(Error::invalid("state must be active or blocked").into());
        }
        if id == admin.id && s == "blocked" {
            return Err(Error::invalid("cannot block yourself").into());
        }
    }
    let current = sqlx::query_as::<_, User>("SELECT * FROM users WHERE id = ?1")
        .bind(id)
        .fetch_optional(&state.db)
        .await?
        .ok_or(Error::NotFound)?;
    let removes_active_admin = current.is_admin
        && current.state == "active"
        && (req.is_admin == Some(false) || req.state.as_deref() == Some("blocked"));
    if removes_active_admin {
        let active_admins: i64 = sqlx::query_scalar(
            "SELECT COUNT(*) FROM users WHERE is_admin = 1 AND state = 'active'",
        )
        .fetch_one(&state.db)
        .await?;
        if active_admins <= 1 {
            return Err(Error::conflict("instance must retain at least one active admin").into());
        }
    }
    let hash = match &req.password {
        Some(p) => {
            password::check_password_policy(p, state.config.auth.min_password_length)?;
            Some(password::hash_password_async(p.clone(), state.config.auth.bcrypt_cost).await?)
        }
        None => None,
    };

    let mut tx = state.db.begin().await?;
    let user = sqlx::query_as::<_, User>(
        r#"
        UPDATE users SET
            name = COALESCE(?1, name),
            email = COALESCE(?2, email),
            is_admin = COALESCE(?3, is_admin),
            state = COALESCE(?4, state),
            password_hash = COALESCE(?5, password_hash),
            updated_at = datetime('now')
        WHERE id = ?6 RETURNING *
        "#,
    )
    .bind(&req.name)
    .bind(&req.email)
    .bind(req.is_admin)
    .bind(&req.state)
    .bind(&hash)
    .bind(id)
    .fetch_optional(&mut *tx)
    .await
    .map_err(map_admin_invariant)?
    .ok_or(Error::NotFound)?;

    // Blocking or resetting a password kills the user's sessions.
    if req.state.as_deref() == Some("blocked") || hash.is_some() {
        sqlx::query("DELETE FROM sessions WHERE user_id = ?1")
            .bind(id)
            .execute(&mut *tx)
            .await?;
    }
    tx.commit().await?;
    Ok(Json(user))
}

/// DELETE /api/v1/admin/users/{id} — refuses when the user still owns projects.
pub async fn delete_user(
    State(state): State<AppState>,
    RequireAdmin(admin): RequireAdmin,
    Path(id): Path<i64>,
) -> ApiResult<StatusCode> {
    if id == admin.id {
        return Err(Error::invalid("cannot delete yourself").into());
    }
    let owned: i64 = sqlx::query_scalar(
        r#"
        SELECT COUNT(*) FROM projects p
        JOIN namespaces n ON n.id = p.namespace_id
        WHERE n.owner_user_id = ?1
        "#,
    )
    .bind(id)
    .fetch_one(&state.db)
    .await?;
    if owned > 0 {
        return Err(
            Error::conflict("user still owns projects; transfer or delete them first").into(),
        );
    }
    let sole_group_owner: i64 = sqlx::query_scalar(
        r#"
        SELECT COUNT(*) FROM group_members mine
        WHERE mine.user_id = ?1 AND mine.access_level = 50
          AND NOT EXISTS (
              SELECT 1 FROM group_members other
              WHERE other.namespace_id = mine.namespace_id
                AND other.user_id <> mine.user_id
                AND other.access_level = 50
          )
        "#,
    )
    .bind(id)
    .fetch_one(&state.db)
    .await?;
    if sole_group_owner > 0 {
        return Err(Error::conflict("user is the sole owner of one or more groups").into());
    }
    let res = sqlx::query("DELETE FROM users WHERE id = ?1")
        .bind(id)
        .execute(&state.db)
        .await
        .map_err(map_admin_invariant)?;
    if res.rows_affected() == 0 {
        return Err(Error::NotFound.into());
    }
    Ok(StatusCode::NO_CONTENT)
}

/// GET /api/v1/admin/projects
pub async fn list_projects(
    State(state): State<AppState>,
    RequireAdmin(_): RequireAdmin,
    axum::extract::Query(page): axum::extract::Query<Pagination>,
) -> ApiResult<Response> {
    let total: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM projects")
        .fetch_one(&state.db)
        .await?;
    let projects =
        sqlx::query_as::<_, Project>("SELECT * FROM projects ORDER BY id LIMIT ?1 OFFSET ?2")
            .bind(page.limit())
            .bind(page.offset())
            .fetch_all(&state.db)
            .await?;
    Ok(paginated_json(projects, total))
}

/// GET /api/v1/admin/stats
pub async fn stats(
    State(state): State<AppState>,
    RequireAdmin(_): RequireAdmin,
) -> ApiResult<Json<serde_json::Value>> {
    let users: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM users")
        .fetch_one(&state.db)
        .await?;
    let projects: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM projects")
        .fetch_one(&state.db)
        .await?;
    let groups: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM namespaces WHERE kind = 'group'")
        .fetch_one(&state.db)
        .await?;
    let lfs: (i64, Option<i64>) = sqlx::query_as("SELECT COUNT(*), SUM(size) FROM lfs_objects")
        .fetch_one(&state.db)
        .await?;
    Ok(Json(serde_json::json!({
        "users": users,
        "projects": projects,
        "groups": groups,
        "lfs_objects": lfs.0,
        "lfs_bytes": lfs.1.unwrap_or(0),
        "version": env!("CARGO_PKG_VERSION"),
    })))
}
