//! Short-lived, project-scoped credentials returned by git-lfs-authenticate.

use crate::auth::token;
use crate::Result;
use rand::Rng;
use sqlx::SqlitePool;

pub const TOKEN_PREFIX: &str = "rgit_lfs_";
const RANDOM_LEN: usize = 40;
const BASE62: &[u8] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789";

pub struct LfsToken {
    pub user_id: i64,
    pub project_id: i64,
    pub can_write: bool,
}

pub async fn issue(
    db: &SqlitePool,
    user_id: i64,
    project_id: i64,
    can_write: bool,
    ttl_seconds: i64,
) -> Result<String> {
    let random: String = {
        let mut rng = rand::thread_rng();
        (0..RANDOM_LEN)
            .map(|_| BASE62[rng.gen_range(0..BASE62.len())] as char)
            .collect()
    };
    let raw = format!("{TOKEN_PREFIX}{random}");
    let expires_at = chrono::Utc::now().naive_utc() + chrono::Duration::seconds(ttl_seconds);
    sqlx::query(
        r#"
        INSERT INTO lfs_auth_tokens (token_hash, user_id, project_id, can_write, expires_at)
        VALUES (?1, ?2, ?3, ?4, ?5)
        "#,
    )
    .bind(token::hash_token(&raw))
    .bind(user_id)
    .bind(project_id)
    .bind(can_write)
    .bind(expires_at)
    .execute(db)
    .await?;
    Ok(raw)
}

pub async fn find(db: &SqlitePool, raw: &str) -> Result<Option<LfsToken>> {
    let row: Option<(i64, i64, bool)> = sqlx::query_as(
        r#"
        SELECT user_id, project_id, can_write
          FROM lfs_auth_tokens
         WHERE token_hash = ?1 AND expires_at > datetime('now')
        "#,
    )
    .bind(token::hash_token(raw))
    .fetch_optional(db)
    .await?;
    Ok(row.map(|(user_id, project_id, can_write)| LfsToken {
        user_id,
        project_id,
        can_write,
    }))
}

pub async fn purge_expired(db: &SqlitePool) -> Result<u64> {
    Ok(
        sqlx::query("DELETE FROM lfs_auth_tokens WHERE expires_at <= datetime('now')")
            .execute(db)
            .await?
            .rows_affected(),
    )
}
