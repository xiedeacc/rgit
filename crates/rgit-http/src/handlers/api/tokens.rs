//! Personal access tokens: /api/v1/user/tokens
//! The raw token is returned exactly once, at creation.

use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::Json;
use rgit_core::auth::token::{self, Scope};
use rgit_core::models::PersonalAccessToken;
use rgit_core::state::AppState;
use rgit_core::Error;
use serde::Deserialize;

use crate::error::ApiResult;
use crate::middleware::auth::RequireUser;

pub async fn list(
    State(state): State<AppState>,
    RequireUser(user, _): RequireUser,
) -> ApiResult<Json<Vec<PersonalAccessToken>>> {
    let tokens = sqlx::query_as::<_, PersonalAccessToken>(
        "SELECT * FROM personal_access_tokens WHERE user_id = ?1 AND revoked = 0 ORDER BY id",
    )
    .bind(user.id)
    .fetch_all(&state.db)
    .await?;
    Ok(Json(tokens))
}

#[derive(Deserialize)]
pub struct CreateToken {
    pub name: String,
    pub scopes: Vec<Scope>,
    /// ISO date (YYYY-MM-DD), optional.
    pub expires_at: Option<String>,
}

pub async fn create(
    State(state): State<AppState>,
    RequireUser(user, _): RequireUser,
    Json(req): Json<CreateToken>,
) -> ApiResult<Response> {
    if req.name.trim().is_empty() {
        return Err(Error::invalid("token name is required").into());
    }
    if req.scopes.is_empty() {
        return Err(Error::invalid("at least one scope is required").into());
    }
    let expires_at = req
        .expires_at
        .as_deref()
        .map(|s| {
            chrono::NaiveDate::parse_from_str(s, "%Y-%m-%d")
                .map(|d| d.and_hms_opt(23, 59, 59).unwrap())
                .map_err(|_| Error::invalid("expires_at must be YYYY-MM-DD"))
        })
        .transpose()?;

    let raw = token::generate_token();
    let row = sqlx::query_as::<_, PersonalAccessToken>(
        r#"
        INSERT INTO personal_access_tokens (user_id, name, token_hash, scopes, expires_at)
        VALUES (?1, ?2, ?3, ?4, ?5) RETURNING *
        "#,
    )
    .bind(user.id)
    .bind(req.name.trim())
    .bind(token::hash_token(&raw))
    .bind(serde_json::to_string(&req.scopes).expect("scopes serialize"))
    .bind(expires_at)
    .fetch_one(&state.db)
    .await?;

    Ok((
        StatusCode::CREATED,
        Json(serde_json::json!({
            "token": row,
            // Shown once; never retrievable again.
            "plaintext": raw,
        })),
    )
        .into_response())
}

pub async fn revoke(
    State(state): State<AppState>,
    RequireUser(user, _): RequireUser,
    Path(id): Path<i64>,
) -> ApiResult<StatusCode> {
    let res = sqlx::query(
        "UPDATE personal_access_tokens SET revoked = 1 WHERE id = ?1 AND user_id = ?2",
    )
    .bind(id)
    .bind(user.id)
    .execute(&state.db)
    .await?;
    if res.rows_affected() == 0 {
        return Err(Error::NotFound.into());
    }
    Ok(StatusCode::NO_CONTENT)
}
