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

/// In-memory login throttle: (login, ip) → (failures, locked_until).
static FAILURES: Mutex<Option<HashMap<String, (u32, Instant)>>> = Mutex::new(None);

fn throttle_key(login: &str, ip: &str) -> String {
    format!("{login}\n{ip}")
}

fn check_lockout(key: &str, max: u32) -> bool {
    let mut guard = FAILURES.lock().unwrap();
    let map = guard.get_or_insert_with(HashMap::new);
    match map.get(key) {
        Some((n, until)) if *n >= max => *until > Instant::now(),
        _ => false,
    }
}

fn record_failure(key: &str, lockout: Duration) {
    let mut guard = FAILURES.lock().unwrap();
    let map = guard.get_or_insert_with(HashMap::new);
    let entry = map.entry(key.to_string()).or_insert((0, Instant::now()));
    entry.0 += 1;
    entry.1 = Instant::now() + lockout;
}

fn clear_failures(key: &str) {
    if let Some(map) = FAILURES.lock().unwrap().as_mut() {
        map.remove(key);
    }
}

/// POST /api/v1/session
pub async fn login(
    State(state): State<AppState>,
    ConnectInfo(addr): ConnectInfo<SocketAddr>,
    headers: HeaderMap,
    Json(req): Json<LoginRequest>,
) -> Result<Response, ApiError> {
    let cfg = &state.config.auth;
    let ip = addr.ip().to_string();
    let key = throttle_key(&req.login, &ip);

    if check_lockout(&key, cfg.max_login_failures) {
        return Err(ApiError(Error::invalid("too many failures, try again later")));
    }

    let user = sqlx::query_as::<_, User>(
        "SELECT * FROM users WHERE (username = ?1 OR email = ?1) AND state = 'active'",
    )
    .bind(&req.login)
    .fetch_optional(&state.db)
    .await?;

    let verified = match &user {
        Some(u) => password::verify_password(&req.password, &u.password_hash),
        None => {
            // Equalize timing for unknown accounts.
            let _ = password::verify_password(&req.password, crate::middleware::auth::dummy_hash());
            false
        }
    };

    if !verified {
        record_failure(&key, Duration::from_secs(cfg.lockout_minutes * 60));
        return Err(ApiError(Error::Unauthorized));
    }
    clear_failures(&key);
    let user = user.expect("verified implies user");

    let raw = session::create_session(
        &state.db,
        user.id,
        cfg.session_ttl_hours,
        Some(&ip),
        headers.get(header::USER_AGENT).and_then(|v| v.to_str().ok()),
    )
    .await?;

    let cookie = format!(
        "{}={raw}; Path=/; HttpOnly; Secure; SameSite=Lax; Max-Age={}",
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
    let clear = format!(
        "{}=; Path=/; HttpOnly; Secure; SameSite=Lax; Max-Age=0",
        session::SESSION_COOKIE
    );
    Ok((StatusCode::NO_CONTENT, [(header::SET_COOKIE, clear)]).into_response())
}

/// GET /api/v1/user
pub async fn current_user(RequireUser(user, _): RequireUser) -> Json<serde_json::Value> {
    Json(serde_json::json!(user))
}
