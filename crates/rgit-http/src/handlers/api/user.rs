//! Current-user profile endpoints.

use axum::extract::State;
use axum::http::StatusCode;
use axum::Json;
use rgit_core::auth::password;
use rgit_core::models::User;
use rgit_core::state::AppState;
use rgit_core::Error;
use serde::Deserialize;

use crate::error::ApiResult;
use crate::middleware::auth::RequireUser;

#[derive(Deserialize)]
pub struct UpdateProfile {
    pub name: Option<String>,
    pub email: Option<String>,
}

/// PATCH /api/v1/user
pub async fn update_profile(
    State(state): State<AppState>,
    RequireUser(user, _): RequireUser,
    Json(req): Json<UpdateProfile>,
) -> ApiResult<Json<User>> {
    if let Some(email) = &req.email {
        if !email.contains('@') {
            return Err(Error::invalid("invalid email").into());
        }
    }
    let updated = sqlx::query_as::<_, User>(
        r#"
        UPDATE users SET
            name = COALESCE(?1, name),
            email = COALESCE(?2, email),
            updated_at = datetime('now')
        WHERE id = ?3 RETURNING *
        "#,
    )
    .bind(&req.name)
    .bind(&req.email)
    .bind(user.id)
    .fetch_one(&state.db)
    .await
    .map_err(|e| match e {
        sqlx::Error::Database(ref d) if d.is_unique_violation() => {
            Error::conflict("email already in use")
        }
        other => Error::Db(other),
    })?;
    Ok(Json(updated))
}

#[derive(Deserialize)]
pub struct ChangePassword {
    pub current_password: String,
    pub new_password: String,
}

/// POST /api/v1/user/password — requires the current password; invalidates
/// all other sessions.
pub async fn change_password(
    State(state): State<AppState>,
    RequireUser(user, _): RequireUser,
    Json(req): Json<ChangePassword>,
) -> ApiResult<StatusCode> {
    if !password::verify_password(&req.current_password, &user.password_hash) {
        return Err(Error::invalid("current password is incorrect").into());
    }
    password::check_password_policy(&req.new_password, state.config.auth.min_password_length)?;
    let hash = password::hash_password(&req.new_password, state.config.auth.bcrypt_cost)?;

    let mut tx = state.db.begin().await?;
    sqlx::query("UPDATE users SET password_hash = ?1, updated_at = datetime('now') WHERE id = ?2")
        .bind(&hash)
        .bind(user.id)
        .execute(&mut *tx)
        .await?;
    sqlx::query("DELETE FROM sessions WHERE user_id = ?1")
        .bind(user.id)
        .execute(&mut *tx)
        .await?;
    tx.commit().await?;
    Ok(StatusCode::NO_CONTENT)
}
