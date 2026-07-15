//! Cookie sessions.
//!
//! The cookie value is 32 random bytes, base64url. The database stores only
//! hex(sha256(value)) so a leaked database cannot be replayed as sessions.

use crate::models::{Session, User};
use crate::Result;
use base64::Engine;
use chrono::{Duration, Utc};
use rand::RngCore;
use sha2::{Digest, Sha256};
use sqlx::SqlitePool;

pub const SESSION_COOKIE: &str = "rgit_session";

fn hash_session_id(raw: &str) -> String {
    hex::encode(Sha256::digest(raw.as_bytes()))
}

/// Create a session; returns the raw cookie value (never stored).
pub async fn create_session(
    db: &SqlitePool,
    user_id: i64,
    ttl_hours: i64,
    ip: Option<&str>,
    user_agent: Option<&str>,
) -> Result<String> {
    let mut bytes = [0u8; 32];
    rand::thread_rng().fill_bytes(&mut bytes);
    let raw = base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(bytes);

    let expires_at = (Utc::now() + Duration::hours(ttl_hours)).naive_utc();
    sqlx::query(
        r#"
        INSERT INTO sessions (id, user_id, expires_at, ip, user_agent)
        VALUES (?1, ?2, ?3, ?4, ?5)
        "#,
    )
    .bind(hash_session_id(&raw))
    .bind(user_id)
    .bind(expires_at)
    .bind(ip)
    .bind(user_agent)
    .execute(db)
    .await?;

    Ok(raw)
}

/// Resolve a raw cookie value to its active user, touching last_seen_at.
pub async fn resolve_session(db: &SqlitePool, raw: &str) -> Result<Option<(Session, User)>> {
    let id = hash_session_id(raw);
    let Some(session) = sqlx::query_as::<_, Session>(
        "SELECT * FROM sessions WHERE id = ?1 AND expires_at > datetime('now')",
    )
    .bind(&id)
    .fetch_optional(db)
    .await?
    else {
        return Ok(None);
    };

    let Some(user) =
        sqlx::query_as::<_, User>("SELECT * FROM users WHERE id = ?1 AND state = 'active'")
            .bind(session.user_id)
            .fetch_optional(db)
            .await?
    else {
        return Ok(None);
    };

    sqlx::query("UPDATE sessions SET last_seen_at = datetime('now') WHERE id = ?1")
        .bind(&id)
        .execute(db)
        .await?;

    Ok(Some((session, user)))
}

pub async fn destroy_session(db: &SqlitePool, raw: &str) -> Result<()> {
    sqlx::query("DELETE FROM sessions WHERE id = ?1")
        .bind(hash_session_id(raw))
        .execute(db)
        .await?;
    Ok(())
}

/// Delete expired sessions (called periodically).
pub async fn purge_expired(db: &SqlitePool) -> Result<u64> {
    let res = sqlx::query("DELETE FROM sessions WHERE expires_at <= datetime('now')")
        .execute(db)
        .await?;
    Ok(res.rows_affected())
}
