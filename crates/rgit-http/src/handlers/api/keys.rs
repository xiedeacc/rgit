//! Self-service SSH key management: /api/v1/user/keys

use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::Json;
use rgit_core::auth::sshkey;
use rgit_core::models::SshKey;
use rgit_core::state::AppState;
use rgit_core::Error;
use serde::Deserialize;

use crate::error::ApiResult;
use crate::middleware::auth::RequireUser;

pub async fn list(
    State(state): State<AppState>,
    RequireUser(user, _): RequireUser,
) -> ApiResult<Json<Vec<SshKey>>> {
    let keys = sqlx::query_as::<_, SshKey>("SELECT * FROM ssh_keys WHERE user_id = ?1 ORDER BY id")
        .bind(user.id)
        .fetch_all(&state.db)
        .await?;
    Ok(Json(keys))
}

#[derive(Deserialize)]
pub struct CreateKey {
    pub title: String,
    pub key: String,
}

pub async fn create(
    State(state): State<AppState>,
    RequireUser(user, _): RequireUser,
    Json(req): Json<CreateKey>,
) -> ApiResult<Response> {
    let parsed = sshkey::parse_public_key(&req.key)?;

    let existing: Option<i64> =
        sqlx::query_scalar("SELECT id FROM ssh_keys WHERE fingerprint_sha256 = ?1")
            .bind(&parsed.fingerprint_sha256)
            .fetch_optional(&state.db)
            .await?;
    if existing.is_some() {
        return Err(Error::conflict("key already registered").into());
    }

    let title = if req.title.trim().is_empty() {
        parsed.comment.clone()
    } else {
        req.title
    };
    let key = sqlx::query_as::<_, SshKey>(
        r#"
        INSERT INTO ssh_keys (user_id, title, key, fingerprint_sha256)
        VALUES (?1, ?2, ?3, ?4) RETURNING *
        "#,
    )
    .bind(user.id)
    .bind(title.trim())
    .bind(&parsed.normalized)
    .bind(&parsed.fingerprint_sha256)
    .fetch_one(&state.db)
    .await?;

    rgit_core::auth::authorized_keys::sync_if_enabled(&state.db, &state.config.ssh).await?;

    Ok((StatusCode::CREATED, Json(key)).into_response())
}

pub async fn delete(
    State(state): State<AppState>,
    RequireUser(user, _): RequireUser,
    Path(id): Path<i64>,
) -> ApiResult<StatusCode> {
    let res = sqlx::query("DELETE FROM ssh_keys WHERE id = ?1 AND user_id = ?2")
        .bind(id)
        .bind(user.id)
        .execute(&state.db)
        .await?;
    if res.rows_affected() == 0 {
        return Err(Error::NotFound.into());
    }
    rgit_core::auth::authorized_keys::sync_if_enabled(&state.db, &state.config.ssh).await?;
    Ok(StatusCode::NO_CONTENT)
}
