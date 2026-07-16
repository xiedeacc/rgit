//! Request identity resolution (DESIGN.md §7).
//!
//! Order: `Authorization: Bearer <PAT>` / `PRIVATE-TOKEN: <PAT>` →
//! `Authorization: Basic user:(password|PAT)` → session cookie.
//! The resolved `Identity` is inserted into request extensions; handlers
//! use the `Identity`/`RequireUser`/`RequireAdmin` extractors.

use axum::extract::{ConnectInfo, FromRequestParts, Request, State};
use axum::http::header;
use axum::http::request::Parts;
use axum::middleware::Next;
use axum::response::{IntoResponse, Response};
use base64::Engine;
use rgit_core::auth::token::{self, Scope};
use rgit_core::auth::{lfs_token, password, session};
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
    LfsToken { project_id: i64, can_write: bool },
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
            Some(AuthMethod::LfsToken { can_write, .. }) => match needed {
                Scope::ReadRepository => true,
                Scope::WriteRepository => *can_write,
                Scope::Api | Scope::ReadApi => false,
            },
            None => false,
        }
    }

    pub fn lfs_project(&self) -> Option<(i64, bool)> {
        match self.method {
            Some(AuthMethod::LfsToken {
                project_id,
                can_write,
            }) => Some((project_id, can_write)),
            _ => None,
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
    let peer = request
        .extensions()
        .get::<ConnectInfo<std::net::SocketAddr>>()
        .map(|ConnectInfo(address)| address.ip());
    let ip = crate::handlers::api::session::client_ip(
        state.config.http.trust_forwarded_headers,
        peer,
        &headers,
    );
    let identity = identify(&state, &headers, &ip).await.unwrap_or_default();
    request.extensions_mut().insert(identity);
    next.run(request).await
}

async fn identify(
    state: &AppState,
    headers: &axum::http::HeaderMap,
    ip: &str,
) -> Result<Identity, Error> {
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
        return authenticate_basic(state, basic, ip).await;
    }

    // 3. Session cookie
    if let Some(raw) = cookie_value(headers, session::SESSION_COOKIE) {
        if let Some((_, user)) = session::resolve_session(&state.db, &raw).await? {
            return Ok(Identity {
                user: Some(user),
                method: Some(AuthMethod::Session),
            });
        }
    }

    Ok(Identity::default())
}

async fn authenticate_pat(state: &AppState, raw: &str) -> Result<Identity, Error> {
    let Some(pat) = token::find_active_token(&state.db, raw).await? else {
        return Ok(Identity::default());
    };
    let user = sqlx::query_as::<_, User>("SELECT * FROM users WHERE id = ?1 AND state = 'active'")
        .bind(pat.user_id)
        .fetch_optional(&state.db)
        .await?;
    let Some(user) = user else {
        return Ok(Identity::default());
    };

    sqlx::query("UPDATE personal_access_tokens SET last_used_at = datetime('now') WHERE id = ?1")
        .bind(pat.id)
        .execute(&state.db)
        .await?;

    Ok(Identity {
        user: Some(user),
        method: Some(AuthMethod::Token {
            scopes: token::parse_scopes(&pat.scopes)?,
        }),
    })
}

async fn authenticate_basic(state: &AppState, encoded: &str, ip: &str) -> Result<Identity, Error> {
    let decoded = base64::engine::general_purpose::STANDARD
        .decode(encoded)
        .map_err(|_| Error::Unauthorized)?;
    let decoded = String::from_utf8(decoded).map_err(|_| Error::Unauthorized)?;
    let Some((login, secret)) = decoded.split_once(':') else {
        return Ok(Identity::default());
    };

    // PAT in the password slot takes precedence (username is then ignored,
    // matching common git-host behavior).
    if secret.starts_with(lfs_token::TOKEN_PREFIX) {
        return authenticate_lfs_token(state, login, secret).await;
    }
    if secret.starts_with(token::TOKEN_PREFIX) {
        return authenticate_pat(state, secret).await;
    }

    let throttle_key = crate::handlers::api::session::auth_throttle_key(login, ip);
    let ip_throttle_key = crate::handlers::api::session::auth_ip_throttle_key(ip);
    if crate::handlers::api::session::check_lockout(
        &throttle_key,
        state.config.auth.max_login_failures,
    ) || crate::handlers::api::session::check_lockout(
        &ip_throttle_key,
        state.config.auth.max_login_failures.saturating_mul(5),
    ) {
        return Ok(Identity::default());
    }

    let user =
        sqlx::query_as::<_, User>("SELECT * FROM users WHERE username = ?1 AND state = 'active'")
            .bind(login)
            .fetch_optional(&state.db)
            .await?;

    let password_hash = user
        .as_ref()
        .map(|user| user.password_hash.clone())
        .unwrap_or_else(|| DUMMY_BCRYPT_HASH.to_string());
    let verified = password::verify_password_async(secret.to_string(), password_hash).await;
    match (user, verified) {
        (Some(user), true) => {
            crate::handlers::api::session::clear_failures(&throttle_key);
            crate::handlers::api::session::clear_failures(&ip_throttle_key);
            Ok(Identity {
                user: Some(user),
                method: Some(AuthMethod::Basic),
            })
        }
        _ => {
            let lockout = std::time::Duration::from_secs(state.config.auth.lockout_minutes * 60);
            crate::handlers::api::session::record_failure(&throttle_key, lockout);
            crate::handlers::api::session::record_failure(&ip_throttle_key, lockout);
            Ok(Identity::default())
        }
    }
}

async fn authenticate_lfs_token(
    state: &AppState,
    login: &str,
    raw: &str,
) -> Result<Identity, Error> {
    let Some(token) = lfs_token::find(&state.db, raw).await? else {
        return Ok(Identity::default());
    };
    let user = sqlx::query_as::<_, User>(
        "SELECT * FROM users WHERE id = ?1 AND username = ?2 AND state = 'active'",
    )
    .bind(token.user_id)
    .bind(login)
    .fetch_optional(&state.db)
    .await?;
    Ok(match user {
        Some(user) => Identity {
            user: Some(user),
            method: Some(AuthMethod::LfsToken {
                project_id: token.project_id,
                can_write: token.can_write,
            }),
        },
        None => Identity::default(),
    })
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
        Ok(parts
            .extensions
            .get::<Identity>()
            .cloned()
            .unwrap_or_default())
    }
}

/// Extractor: 401 unless authenticated and active.
pub struct RequireUser(pub User, pub Identity);

impl<S: Send + Sync> FromRequestParts<S> for RequireUser {
    type Rejection = ApiError;

    async fn from_request_parts(parts: &mut Parts, _s: &S) -> Result<Self, Self::Rejection> {
        let identity = parts
            .extensions
            .get::<Identity>()
            .cloned()
            .unwrap_or_default();
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

/// API authorization guard.
///
/// Session callers must send the exact `X-Rgit-Csrf: 1` header on mutations.
/// PAT callers additionally need `read_api` for safe methods and `api` for
/// mutations. Repository-only scopes are intentionally valid only on the Git
/// and LFS route tree, which does not use this middleware.
pub async fn api_guard(request: Request, next: Next) -> Response {
    let mutating = matches!(
        *request.method(),
        axum::http::Method::POST
            | axum::http::Method::PATCH
            | axum::http::Method::PUT
            | axum::http::Method::DELETE
    );
    if let Some(identity) = request.extensions().get::<Identity>() {
        match &identity.method {
            Some(AuthMethod::Session) if mutating => {
                let valid_csrf = request
                    .headers()
                    .get("X-Rgit-Csrf")
                    .and_then(|value| value.to_str().ok())
                    == Some("1");
                if !valid_csrf {
                    return ApiError(Error::Forbidden).into_response();
                }
            }
            Some(AuthMethod::Token { scopes }) => {
                let needed = if mutating { Scope::Api } else { Scope::ReadApi };
                if !token::scopes_allow(scopes, needed) {
                    return ApiError(Error::Forbidden).into_response();
                }
            }
            _ => {}
        }
    }
    next.run(request).await
}
