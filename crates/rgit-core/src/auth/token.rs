//! Personal access tokens.
//!
//! Format: `rgit_` + 40 random base62 chars (~238 bits). Only
//! hex(sha256(token)) is persisted; the raw token is shown exactly once.

use crate::models::PersonalAccessToken;
use crate::{Error, Result};
use rand::Rng;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use sqlx::SqlitePool;

pub const TOKEN_PREFIX: &str = "rgit_";
const TOKEN_RANDOM_LEN: usize = 40;
const BASE62: &[u8] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789";

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Scope {
    /// Full API read/write + repository read/write.
    Api,
    /// Read-only API.
    ReadApi,
    /// git fetch/clone + LFS download over HTTP(S).
    ReadRepository,
    /// git push + LFS upload over HTTP(S) (implies read_repository).
    WriteRepository,
}

pub fn generate_token() -> String {
    let mut rng = rand::thread_rng();
    let body: String = (0..TOKEN_RANDOM_LEN)
        .map(|_| BASE62[rng.gen_range(0..BASE62.len())] as char)
        .collect();
    format!("{TOKEN_PREFIX}{body}")
}

pub fn hash_token(token: &str) -> String {
    hex::encode(Sha256::digest(token.as_bytes()))
}

/// Look up an unexpired, unrevoked token by its raw value.
/// Lookup is by sha256 digest, so no timing side channel on the raw token.
pub async fn find_active_token(
    db: &SqlitePool,
    raw: &str,
) -> Result<Option<PersonalAccessToken>> {
    let hash = hash_token(raw);
    let token = sqlx::query_as::<_, PersonalAccessToken>(
        r#"
        SELECT * FROM personal_access_tokens
         WHERE token_hash = ?1
           AND revoked = 0
           AND (expires_at IS NULL OR expires_at > datetime('now'))
        "#,
    )
    .bind(hash)
    .fetch_optional(db)
    .await?;
    Ok(token)
}

pub fn parse_scopes(json: &str) -> Result<Vec<Scope>> {
    serde_json::from_str(json).map_err(|_| Error::invalid("malformed token scopes"))
}

pub fn scopes_allow(scopes: &[Scope], needed: Scope) -> bool {
    scopes.iter().any(|s| {
        *s == needed
            || matches!(
                (s, needed),
                (Scope::Api, _)
                    | (Scope::WriteRepository, Scope::ReadRepository)
                    | (Scope::ReadApi, Scope::ReadApi)
            )
    })
}
