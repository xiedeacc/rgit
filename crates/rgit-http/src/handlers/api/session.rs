//! Login / logout / current user (DESIGN.md §7.2, §10).

use axum::extract::{ConnectInfo, State};
use axum::http::{header, HeaderMap, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::Json;
use rgit_core::auth::{password, session};
use rgit_core::models::User;
use rgit_core::state::AppState;
use rgit_core::Error;
use serde::Deserialize;
use std::collections::HashMap;
use std::net::SocketAddr;
use std::sync::Mutex;
use std::time::{Duration, Instant};

use crate::error::{ApiError, ApiResult};
use crate::middleware::auth::RequireUser;

#[derive(Deserialize)]
pub struct LoginRequest {
    pub login: String,
    pub password: String,
}

/// In-memory login throttle: identity and IP keys → (failures, locked_until).
static FAILURES: Mutex<Option<HashMap<String, (u32, Instant)>>> = Mutex::new(None);
const MAX_FAILURE_KEYS: usize = 10_000;

fn throttle_key(login: &str, ip: &str) -> String {
    format!("login\n{}\n{ip}", login.to_ascii_lowercase())
}

pub(crate) fn check_lockout(key: &str, max: u32) -> bool {
    let mut guard = FAILURES
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    let map = guard.get_or_insert_with(HashMap::new);
    let now = Instant::now();
    map.retain(|_, (_, expires)| *expires > now);
    match map.get(key) {
        Some((n, until)) if *n >= max => *until > now,
        _ => false,
    }
}

pub(crate) fn record_failure(key: &str, lockout: Duration) {
    let mut guard = FAILURES
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    let map = guard.get_or_insert_with(HashMap::new);
    let now = Instant::now();
    map.retain(|_, (_, expires)| *expires > now);
    if map.len() >= MAX_FAILURE_KEYS && !map.contains_key(key) {
        if let Some(evicted) = map.keys().next().cloned() {
            map.remove(&evicted);
        }
    }
    let entry = map.entry(key.to_string()).or_insert((0, Instant::now()));
    entry.0 += 1;
    entry.1 = now + lockout;
}

pub(crate) fn clear_failures(key: &str) {
    if let Some(map) = FAILURES
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
        .as_mut()
    {
        map.remove(key);
    }
}

pub(crate) fn auth_throttle_key(login: &str, ip: &str) -> String {
    throttle_key(login, ip)
}

pub(crate) fn auth_ip_throttle_key(ip: &str) -> String {
    format!("ip\n{ip}")
}

pub(crate) fn client_ip(
    trust_forwarded: bool,
    peer: Option<std::net::IpAddr>,
    headers: &HeaderMap,
) -> String {
    if trust_forwarded && peer.is_some_and(|ip| ip.is_loopback()) {
        let candidate = headers
            .get("X-Real-IP")
            .and_then(|value| value.to_str().ok())
            .or_else(|| {
                headers
                    .get("X-Forwarded-For")
                    .and_then(|value| value.to_str().ok())
                    .and_then(|value| value.split(',').next())
            })
            .map(str::trim);
        if let Some(ip) = candidate.and_then(|value| value.parse::<std::net::IpAddr>().ok()) {
            return ip.to_string();
        }
    }
    peer.map(|ip| ip.to_string())
        .unwrap_or_else(|| "unknown".into())
}

/// POST /api/v1/session
pub async fn login(
    State(state): State<AppState>,
    ConnectInfo(addr): ConnectInfo<SocketAddr>,
    headers: HeaderMap,
    Json(req): Json<LoginRequest>,
) -> Result<Response, ApiError> {
    let cfg = &state.config.auth;
    let ip = client_ip(
        state.config.http.trust_forwarded_headers,
        Some(addr.ip()),
        &headers,
    );
    let key = throttle_key(&req.login, &ip);
    let ip_key = auth_ip_throttle_key(&ip);

    if check_lockout(&key, cfg.max_login_failures)
        || check_lockout(&ip_key, cfg.max_login_failures.saturating_mul(5))
    {
        return Ok((
            StatusCode::TOO_MANY_REQUESTS,
            Json(serde_json::json!({
                "error": "rate_limited",
                "message": "too many failures, try again later"
            })),
        )
            .into_response());
    }

    let user = sqlx::query_as::<_, User>(
        "SELECT * FROM users WHERE (username = ?1 OR email = ?1) AND state = 'active'",
    )
    .bind(&req.login)
    .fetch_optional(&state.db)
    .await?;

    let password_hash = user
        .as_ref()
        .map(|user| user.password_hash.clone())
        .unwrap_or_else(|| crate::middleware::auth::dummy_hash().to_string());
    let verified = password::verify_password_async(req.password.clone(), password_hash).await
        && user.is_some();

    if !verified {
        let lockout = Duration::from_secs(cfg.lockout_minutes * 60);
        record_failure(&key, lockout);
        record_failure(&ip_key, lockout);
        return Err(ApiError(Error::Unauthorized));
    }
    clear_failures(&key);
    clear_failures(&ip_key);
    let user = user.expect("verified implies user");

    let raw = session::create_session(
        &state.db,
        user.id,
        cfg.session_ttl_hours,
        Some(&ip),
        headers
            .get(header::USER_AGENT)
            .and_then(|v| v.to_str().ok()),
    )
    .await?;

    let secure = if state.config.http.external_url.starts_with("https://") {
        "; Secure"
    } else {
        ""
    };
    let cookie = format!(
        "{}={raw}; Path=/; HttpOnly{secure}; SameSite=Strict; Max-Age={}",
        session::SESSION_COOKIE,
        state.config.auth.session_ttl_hours * 3600
    );
    Ok((
        StatusCode::OK,
        [(header::SET_COOKIE, cookie)],
        Json(serde_json::json!({ "user": user })),
    )
        .into_response())
}

/// DELETE /api/v1/session
pub async fn logout(State(state): State<AppState>, headers: HeaderMap) -> ApiResult<Response> {
    if let Some(raw) = headers
        .get(header::COOKIE)
        .and_then(|v| v.to_str().ok())
        .and_then(|c| {
            c.split(';')
                .filter_map(|kv| kv.trim().split_once('='))
                .find(|(k, _)| *k == session::SESSION_COOKIE)
                .map(|(_, v)| v.to_string())
        })
    {
        session::destroy_session(&state.db, &raw).await?;
    }
    let secure = if state.config.http.external_url.starts_with("https://") {
        "; Secure"
    } else {
        ""
    };
    let clear = format!(
        "{}=; Path=/; HttpOnly{secure}; SameSite=Strict; Max-Age=0",
        session::SESSION_COOKIE,
    );
    Ok((StatusCode::NO_CONTENT, [(header::SET_COOKIE, clear)]).into_response())
}

/// GET /api/v1/user
pub async fn current_user(RequireUser(user, _): RequireUser) -> Json<serde_json::Value> {
    Json(serde_json::json!(user))
}
