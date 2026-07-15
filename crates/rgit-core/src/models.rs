//! Database row types. Field names mirror the schema in migrations/0001_init.sql.

use chrono::NaiveDateTime;
use serde::Serialize;
use sqlx::FromRow;

#[derive(Debug, Clone, FromRow, Serialize)]
pub struct User {
    pub id: i64,
    pub username: String,
    pub email: String,
    pub name: String,
    #[serde(skip_serializing)]
    pub password_hash: String,
    pub is_admin: bool,
    pub state: String,
    pub created_at: NaiveDateTime,
    pub updated_at: NaiveDateTime,
}

impl User {
    pub fn is_active(&self) -> bool {
        self.state == "active"
    }
}

#[derive(Debug, Clone, FromRow, Serialize)]
pub struct Namespace {
    pub id: i64,
    pub path: String,
    pub name: String,
    pub kind: String, // "user" | "group"
    pub owner_user_id: Option<i64>,
    pub description: String,
    pub created_at: NaiveDateTime,
    pub updated_at: NaiveDateTime,
}

#[derive(Debug, Clone, FromRow, Serialize)]
pub struct Project {
    pub id: i64,
    pub namespace_id: i64,
    pub path: String,
    pub name: String,
    pub description: String,
    pub visibility: i32,
    pub default_branch: Option<String>,
    pub archived: bool,
    pub lfs_enabled: bool,
    pub disk_id: i64,
    pub disk_hash: String,
    pub forked_from_project_id: Option<i64>,
    pub created_at: NaiveDateTime,
    pub updated_at: NaiveDateTime,
}

#[derive(Debug, Clone, FromRow, Serialize)]
pub struct ProjectMember {
    pub project_id: i64,
    pub user_id: i64,
    pub access_level: i32,
    pub created_at: NaiveDateTime,
}

#[derive(Debug, Clone, FromRow, Serialize)]
pub struct GroupMember {
    pub namespace_id: i64,
    pub user_id: i64,
    pub access_level: i32,
    pub created_at: NaiveDateTime,
}

#[derive(Debug, Clone, FromRow, Serialize)]
pub struct SshKey {
    pub id: i64,
    pub user_id: i64,
    pub title: String,
    pub key: String,
    pub fingerprint_sha256: String,
    pub created_at: NaiveDateTime,
    pub last_used_at: Option<NaiveDateTime>,
}

#[derive(Debug, Clone, FromRow, Serialize)]
pub struct PersonalAccessToken {
    pub id: i64,
    pub user_id: i64,
    pub name: String,
    #[serde(skip_serializing)]
    pub token_hash: String,
    pub scopes: String, // JSON array
    pub expires_at: Option<NaiveDateTime>,
    pub last_used_at: Option<NaiveDateTime>,
    pub revoked: bool,
    pub created_at: NaiveDateTime,
}

#[derive(Debug, Clone, FromRow)]
pub struct Session {
    pub id: String,
    pub user_id: i64,
    pub created_at: NaiveDateTime,
    pub expires_at: NaiveDateTime,
    pub last_seen_at: Option<NaiveDateTime>,
    pub ip: Option<String>,
    pub user_agent: Option<String>,
}

#[derive(Debug, Clone, FromRow, Serialize)]
pub struct LfsObject {
    pub id: i64,
    pub oid: String,
    pub size: i64,
    pub created_at: NaiveDateTime,
}
