//! Request identity resolution (DESIGN.md §7).
//!
//! Order: `Authorization: Bearer <PAT>` / `PRIVATE-TOKEN: <PAT>` →
//! `Authorization: Basic user:(password|PAT)` → session cookie.
//! The resolved `Identity` is inserted into request extensions; handlers
//! use the `Identity`/`RequireUser`/`RequireAdmin` extractors.

use axum::extract::{FromRequestParts, Request, State};
use axum::http::header;
use axum::http::request::Parts;
use axum::middleware::Next;
use axum::response::{IntoResponse, Response};
use base64::Engine;
use rgit_core::auth::token::{self, Scope};
use rgit_core::auth::{password, session};
use rgit_core::models::User;
use rgit_core::state::AppState;
use rgit_core::Error;

use crate::error::ApiError;

/// How the caller authenticated — checked against per-route requirements
/// (e.g. PAT scopes apply only to token auth).
#[derive(Debug, Clone)]
pub enum AuthMethod {
    Session,
    Token { scopes: Vec<Scope> },
    Basic,
}

#[derive(Debug, Clone, Default)]
pub struct Identity {
    pub user: Option<User>,
    pub method: Option<AuthMethod>,
}

impl Identity {
    pub fn allows(&self, needed: Scope) -> bool {
        match &self.method {
            // Session/basic-password auth carries full user authority.
            Some(AuthMethod::Session) | Some(AuthMethod::Basic) => true,
            Some(AuthMethod::Token { scopes }) => token::scopes_allow(scopes, needed),
            None => false,
        }
    }
}

/// Middleware: resolve identity (never rejects; handlers decide).
pub async fn resolve_identity(
    State(state): State<AppState>,
    mut request: Request,
    next: Next,
) -> Response {
    // Only headers cross the await: Body is !Sync, HeaderMap is Sync.
    let headers = request.headers().clone();
    let identity = identify(&state, &headers).await.unwrap_or_default();
    request.extensions_mut().insert(identity);
    next.run(request).await
}

async fn identify(state: &AppState, headers: &axum::http::HeaderMap) -> Result<Identity, Error> {
    // 1. PRIVATE-TOKEN / Bearer PAT
    let raw_token = headers
        .get("PRIVATE-TOKEN")
        .and_then(|v| v.to_str().ok())
        .map(str::to_string)
        .or_else(|| {
            headers
                .get(header::AUTHORIZATION)
                .and_then(|v| v.to_str().ok())
                .and_then(|v| v.strip_prefix("Bearer "))
                .map(str::to_string)
        });
    if let Some(raw) = raw_token {
        return authenticate_pat(state, &raw).await;
    }

    // 2. Basic auth: password or PAT in the password slot (git clients)
    if let Some(basic) = headers
        .get(header::AUTHORIZATION)
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.strip_prefix("Basic "))
    {
        return authenticate_basic(state, basic).await;
    }

    // 3. Session cookie
    if let Some(raw) = cookie_value(headers, session::SESSION_COOKIE) {
        if let Some((_, user)) = session::resolve_session(&state.db, &raw).await? {
            return Ok(Identity { user: Some(user), method: Some(AuthMethod::Session) });
        }
    }

    Ok(Identity::default())
}

async fn authenticate_pat(state: &AppState, raw: &str) -> Result<Identity, Error> {
    let Some(pat) = token::find_active_token(&state.db, raw).await? else {
        return Ok(Identity::default());
    };
    let user = sqlx::query_as::<_, User>(
        "SELECT * FROM users WHERE id = ?1 AND state = 'active'",
    )
    .bind(pat.user_id)
    .fetch_optional(&state.db)
    .await?;
    let Some(user) = user else { return Ok(Identity::default()) };

    sqlx::query("UPDATE personal_access_tokens SET last_used_at = datetime('now') WHERE id = ?1")
        .bind(pat.id)
        .execute(&state.db)
        .await?;

    Ok(Identity {
        user: Some(user),
        method: Some(AuthMethod::Token { scopes: token::parse_scopes(&pat.scopes)? }),
    })
}

async fn authenticate_basic(state: &AppState, encoded: &str) -> Result<Identity, Error> {
    let decoded = base64::engine::general_purpose::STANDARD
        .decode(encoded)
        .map_err(|_| Error::Unauthorized)?;
    let decoded = String::from_utf8(decoded).map_err(|_| Error::Unauthorized)?;
    let Some((login, secret)) = decoded.split_once(':') else {
        return Ok(Identity::default());
    };

    // PAT in the password slot takes precedence (username is then ignored,
    // matching common git-host behavior).
    if secret.starts_with(token::TOKEN_PREFIX) {
        return authenticate_pat(state, secret).await;
    }

    let user = sqlx::query_as::<_, User>(
        "SELECT * FROM users WHERE username = ?1 AND state = 'active'",
    )
    .bind(login)
    .fetch_optional(&state.db)
    .await?;

    match user {
        Some(user) if password::verify_password(secret, &user.password_hash) => {
            Ok(Identity { user: Some(user), method: Some(AuthMethod::Basic) })
        }
        _ => {
            // Constant-ish time: burn a bcrypt verification on unknown users too.
            let _ = password::verify_password(secret, DUMMY_BCRYPT_HASH);
            Ok(Identity::default())
        }
    }
}

/// bcrypt hash of an unguessable throwaway value, used to equalize timing.
const DUMMY_BCRYPT_HASH: &str = "$2b$12$C6UzMDM.H6dfI/f/IKcEeO7ZBpDLhAuBIrEmSHDPMU0.PEZFXvyzC";

/// For handlers that need to burn a bcrypt verification on unknown accounts.
pub fn dummy_hash() -> &'static str {
    DUMMY_BCRYPT_HASH
}

fn cookie_value(headers: &axum::http::HeaderMap, name: &str) -> Option<String> {
    headers
        .get(header::COOKIE)?
        .to_str()
        .ok()?
        .split(';')
        .filter_map(|kv| kv.trim().split_once('='))
        .find(|(k, _)| *k == name)
        .map(|(_, v)| v.to_string())
}

/// Extractor: optional identity (always succeeds).
impl<S: Send + Sync> FromRequestParts<S> for Identity {
    type Rejection = std::convert::Infallible;

    async fn from_request_parts(parts: &mut Parts, _s: &S) -> Result<Self, Self::Rejection> {
        Ok(parts.extensions.get::<Identity>().cloned().unwrap_or_default())
    }
}

/// Extractor: 401 unless authenticated and active.
pub struct RequireUser(pub User, pub Identity);

impl<S: Send + Sync> FromRequestParts<S> for RequireUser {
    type Rejection = ApiError;

    async fn from_request_parts(parts: &mut Parts, _s: &S) -> Result<Self, Self::Rejection> {
        let identity = parts.extensions.get::<Identity>().cloned().unwrap_or_default();
        match identity.user.clone() {
            Some(user) => Ok(RequireUser(user, identity)),
            None => Err(ApiError(Error::Unauthorized)),
        }
    }
}

/// Extractor: 403 unless admin.
pub struct RequireAdmin(pub User);

impl<S: Send + Sync> FromRequestParts<S> for RequireAdmin {
    type Rejection = ApiError;

    async fn from_request_parts(parts: &mut Parts, s: &S) -> Result<Self, Self::Rejection> {
        let RequireUser(user, _) = RequireUser::from_request_parts(parts, s).await?;
        if user.is_admin {
            Ok(RequireAdmin(user))
        } else {
            Err(ApiError(Error::Forbidden))
        }
    }
}

/// CSRF guard for cookie-authenticated mutating requests (DESIGN.md §7.2):
/// session callers must send `X-Rgit-Csrf: 1`. Token/basic auth is exempt
/// (no ambient browser credential).
pub async fn csrf_guard(request: Request, next: Next) -> Response {
    let mutating = matches!(
        *request.method(),
        axum::http::Method::POST | axum::http::Method::PATCH
            | axum::http::Method::PUT | axum::http::Method::DELETE
    );
    if mutating {
        let is_session = matches!(
            request.extensions().get::<Identity>(),
            Some(Identity { method: Some(AuthMethod::Session), .. })
        );
        if is_session && request.headers().get("X-Rgit-Csrf").is_none() {
            return ApiError(Error::Forbidden).into_response();
        }
    }
    next.run(request).await
}
