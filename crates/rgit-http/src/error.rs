//! rgit_core::Error → HTTP response mapping. Error body shape (DESIGN.md §10):
//! {"error": "<kind>", "message": "<detail>"}

use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::Json;
use rgit_core::Error;

pub struct ApiError(pub Error);

pub type ApiResult<T> = std::result::Result<T, ApiError>;

impl From<Error> for ApiError {
    fn from(e: Error) -> Self {
        ApiError(e)
    }
}

impl From<sqlx::Error> for ApiError {
    fn from(e: sqlx::Error) -> Self {
        ApiError(Error::Db(e))
    }
}

impl IntoResponse for ApiError {
    fn into_response(self) -> Response {
        let (status, kind) = match &self.0 {
            Error::NotFound => (StatusCode::NOT_FOUND, "not_found"),
            Error::Unauthorized => (StatusCode::UNAUTHORIZED, "unauthorized"),
            Error::Forbidden => (StatusCode::FORBIDDEN, "forbidden"),
            Error::Conflict(_) => (StatusCode::CONFLICT, "conflict"),
            Error::Invalid(_) => (StatusCode::BAD_REQUEST, "invalid"),
            Error::Db(_) | Error::Io(_) | Error::Git(_) | Error::Internal(_) => {
                tracing::error!(error = %self.0, "internal error");
                (StatusCode::INTERNAL_SERVER_ERROR, "internal")
            }
        };
        // Never leak internal error details to clients.
        let message = if status == StatusCode::INTERNAL_SERVER_ERROR {
            "internal server error".to_string()
        } else {
            self.0.to_string()
        };
        (
            status,
            Json(serde_json::json!({ "error": kind, "message": message })),
        )
            .into_response()
    }
}

/// Placeholder for endpoints scaffolded but not yet implemented (M2+).
pub fn not_implemented() -> Response {
    (
        StatusCode::NOT_IMPLEMENTED,
        Json(serde_json::json!({
            "error": "not_implemented",
            "message": "endpoint scaffolded, implementation pending"
        })),
    )
        .into_response()
}
