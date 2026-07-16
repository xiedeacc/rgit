CREATE TABLE lfs_auth_tokens (
    token_hash TEXT PRIMARY KEY,
    user_id    INTEGER NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    project_id INTEGER NOT NULL REFERENCES projects(id) ON DELETE CASCADE,
    can_write  INTEGER NOT NULL,
    expires_at TEXT NOT NULL
);
CREATE INDEX idx_lfs_auth_tokens_expires ON lfs_auth_tokens(expires_at);
