-- rgit initial schema.
-- Conventions:
--   * integer constants (visibility, access_level) match GitLab so migration is a copy
--   * timestamps are TEXT, ISO-8601 UTC ("YYYY-MM-DD HH:MM:SS")
--   * booleans are INTEGER 0/1

CREATE TABLE users (
    id            INTEGER PRIMARY KEY,
    username      TEXT NOT NULL UNIQUE COLLATE NOCASE,
    email         TEXT NOT NULL UNIQUE COLLATE NOCASE,
    name          TEXT NOT NULL DEFAULT '',
    -- bcrypt hash; GitLab users.encrypted_password (Devise bcrypt) is copied verbatim
    -- so migrated users keep their passwords.
    password_hash TEXT NOT NULL,
    is_admin      INTEGER NOT NULL DEFAULT 0,
    -- 'active' | 'blocked'
    state         TEXT NOT NULL DEFAULT 'active',
    created_at    TEXT NOT NULL DEFAULT (datetime('now')),
    updated_at    TEXT NOT NULL DEFAULT (datetime('now'))
);

-- URL namespace a project lives under: every user has one (kind='user'),
-- plus explicitly created shared groups (kind='group').
CREATE TABLE namespaces (
    id            INTEGER PRIMARY KEY,
    path          TEXT NOT NULL UNIQUE COLLATE NOCASE,
    name          TEXT NOT NULL,
    kind          TEXT NOT NULL CHECK (kind IN ('user', 'group')),
    owner_user_id INTEGER REFERENCES users(id) ON DELETE CASCADE,
    description   TEXT NOT NULL DEFAULT '',
    created_at    TEXT NOT NULL DEFAULT (datetime('now')),
    updated_at    TEXT NOT NULL DEFAULT (datetime('now'))
);

CREATE TABLE group_members (
    namespace_id INTEGER NOT NULL REFERENCES namespaces(id) ON DELETE CASCADE,
    user_id      INTEGER NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    -- 10 guest / 20 reporter / 30 developer / 40 maintainer / 50 owner
    access_level INTEGER NOT NULL,
    created_at   TEXT NOT NULL DEFAULT (datetime('now')),
    PRIMARY KEY (namespace_id, user_id)
);

CREATE TABLE projects (
    id                     INTEGER PRIMARY KEY,
    namespace_id           INTEGER NOT NULL REFERENCES namespaces(id),
    path                   TEXT NOT NULL COLLATE NOCASE,
    name                   TEXT NOT NULL,
    description            TEXT NOT NULL DEFAULT '',
    -- 0 private / 10 internal / 20 public
    visibility             INTEGER NOT NULL DEFAULT 0,
    default_branch         TEXT,
    archived               INTEGER NOT NULL DEFAULT 0,
    lfs_enabled            INTEGER NOT NULL DEFAULT 1,
    -- Immutable id that determines the on-disk hashed-storage path.
    -- Equals the original GitLab project id for migrated projects.
    disk_id                INTEGER NOT NULL UNIQUE,
    -- hex(sha256(disk_id)) — cached so paths never depend on recomputation.
    disk_hash              TEXT NOT NULL UNIQUE,
    forked_from_project_id INTEGER REFERENCES projects(id) ON DELETE SET NULL,
    created_at             TEXT NOT NULL DEFAULT (datetime('now')),
    updated_at             TEXT NOT NULL DEFAULT (datetime('now')),
    UNIQUE (namespace_id, path)
);
CREATE INDEX idx_projects_namespace ON projects(namespace_id);
CREATE INDEX idx_projects_visibility ON projects(visibility);

CREATE TABLE project_members (
    project_id   INTEGER NOT NULL REFERENCES projects(id) ON DELETE CASCADE,
    user_id      INTEGER NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    access_level INTEGER NOT NULL,
    created_at   TEXT NOT NULL DEFAULT (datetime('now')),
    PRIMARY KEY (project_id, user_id)
);
CREATE INDEX idx_project_members_user ON project_members(user_id);

CREATE TABLE ssh_keys (
    id                 INTEGER PRIMARY KEY,
    user_id            INTEGER NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    title              TEXT NOT NULL DEFAULT '',
    -- Full "<algo> <base64> [comment]" public key line.
    key                TEXT NOT NULL,
    -- base64 (unpadded) sha256 fingerprint, GitLab fingerprint_sha256 format.
    fingerprint_sha256 TEXT NOT NULL UNIQUE,
    created_at         TEXT NOT NULL DEFAULT (datetime('now')),
    last_used_at       TEXT
);
CREATE INDEX idx_ssh_keys_user ON ssh_keys(user_id);

CREATE TABLE personal_access_tokens (
    id           INTEGER PRIMARY KEY,
    user_id      INTEGER NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    name         TEXT NOT NULL,
    -- hex(sha256(token)); the raw token ("rgit_..." format) is shown once at creation.
    token_hash   TEXT NOT NULL UNIQUE,
    -- JSON array, subset of ["api","read_api","read_repository","write_repository"]
    scopes       TEXT NOT NULL,
    expires_at   TEXT,
    last_used_at TEXT,
    revoked      INTEGER NOT NULL DEFAULT 0,
    created_at   TEXT NOT NULL DEFAULT (datetime('now'))
);
CREATE INDEX idx_pat_user ON personal_access_tokens(user_id);

CREATE TABLE sessions (
    id           TEXT PRIMARY KEY, -- hex(sha256(cookie value)); raw value never stored
    user_id      INTEGER NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    created_at   TEXT NOT NULL DEFAULT (datetime('now')),
    expires_at   TEXT NOT NULL,
    last_seen_at TEXT,
    ip           TEXT,
    user_agent   TEXT
);
CREATE INDEX idx_sessions_user ON sessions(user_id);
CREATE INDEX idx_sessions_expires ON sessions(expires_at);

-- Content-addressed LFS store (same dedup model as GitLab: objects are global,
-- linked to projects through a join table).
CREATE TABLE lfs_objects (
    id         INTEGER PRIMARY KEY,
    oid        TEXT NOT NULL UNIQUE, -- lowercase hex sha256 of content
    size       INTEGER NOT NULL,
    created_at TEXT NOT NULL DEFAULT (datetime('now'))
);

CREATE TABLE project_lfs_objects (
    project_id    INTEGER NOT NULL REFERENCES projects(id) ON DELETE CASCADE,
    lfs_object_id INTEGER NOT NULL REFERENCES lfs_objects(id) ON DELETE CASCADE,
    PRIMARY KEY (project_id, lfs_object_id)
);
CREATE INDEX idx_project_lfs_lfs ON project_lfs_objects(lfs_object_id);
