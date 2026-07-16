//! Database row types. Field names mirror the schema in migrations/0001_init.sql.

use chrono::NaiveDateTime;
use serde::Serialize;
use sqlx::{sqlite::SqliteRow, FromRow, Row};

#[derive(Debug, Clone, Serialize)]
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

#[derive(Debug, Clone, Serialize)]
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

#[derive(Debug, Clone, Serialize)]
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

#[derive(Debug, Clone, Serialize)]
pub struct ProjectMember {
    pub project_id: i64,
    pub user_id: i64,
    pub access_level: i32,
    pub created_at: NaiveDateTime,
}

#[derive(Debug, Clone, Serialize)]
pub struct GroupMember {
    pub namespace_id: i64,
    pub user_id: i64,
    pub access_level: i32,
    pub created_at: NaiveDateTime,
}

#[derive(Debug, Clone, Serialize)]
pub struct SshKey {
    pub id: i64,
    pub user_id: i64,
    pub title: String,
    pub key: String,
    pub fingerprint_sha256: String,
    pub created_at: NaiveDateTime,
    pub last_used_at: Option<NaiveDateTime>,
}

#[derive(Debug, Clone, Serialize)]
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

#[derive(Debug, Clone)]
pub struct Session {
    pub id: String,
    pub user_id: i64,
    pub created_at: NaiveDateTime,
    pub expires_at: NaiveDateTime,
    pub last_seen_at: Option<NaiveDateTime>,
    pub ip: Option<String>,
    pub user_agent: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct LfsObject {
    pub id: i64,
    pub oid: String,
    pub size: i64,
    pub created_at: NaiveDateTime,
}

macro_rules! impl_sqlite_row {
    ($model:ident { $($field:ident),+ $(,)? }) => {
        impl<'r> FromRow<'r, SqliteRow> for $model {
            fn from_row(row: &'r SqliteRow) -> std::result::Result<Self, sqlx::Error> {
                Ok(Self {
                    $($field: row.try_get(stringify!($field))?,)+
                })
            }
        }
    };
}

impl_sqlite_row!(User {
    id,
    username,
    email,
    name,
    password_hash,
    is_admin,
    state,
    created_at,
    updated_at,
});
impl_sqlite_row!(Namespace {
    id,
    path,
    name,
    kind,
    owner_user_id,
    description,
    created_at,
    updated_at,
});
impl_sqlite_row!(Project {
    id,
    namespace_id,
    path,
    name,
    description,
    visibility,
    default_branch,
    archived,
    lfs_enabled,
    disk_id,
    disk_hash,
    forked_from_project_id,
    created_at,
    updated_at,
});
impl_sqlite_row!(ProjectMember {
    project_id,
    user_id,
    access_level,
    created_at,
});
impl_sqlite_row!(GroupMember {
    namespace_id,
    user_id,
    access_level,
    created_at,
});
impl_sqlite_row!(SshKey {
    id,
    user_id,
    title,
    key,
    fingerprint_sha256,
    created_at,
    last_used_at,
});
impl_sqlite_row!(PersonalAccessToken {
    id,
    user_id,
    name,
    token_hash,
    scopes,
    expires_at,
    last_used_at,
    revoked,
    created_at,
});
impl_sqlite_row!(Session {
    id,
    user_id,
    created_at,
    expires_at,
    last_seen_at,
    ip,
    user_agent,
});
impl_sqlite_row!(LfsObject {
    id,
    oid,
    size,
    created_at,
});
