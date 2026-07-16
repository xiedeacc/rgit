#!/usr/bin/env bash
set -euo pipefail

for command in cargo git psql runuser sqlite3 sha256sum; do
    if ! command -v "$command" >/dev/null 2>&1; then
        echo "migration integration test skipped: missing $command"
        exit 0
    fi
done
if ! pg_isready >/dev/null 2>&1; then
    echo "migration integration test skipped: PostgreSQL is not running"
    exit 0
fi

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
tmp="$(mktemp -d)"
suffix="$$"
db="rgit_migrate_test_${suffix}"
role="rgit_migrate_test_${suffix}"
password="rgit-test-${suffix}"

cleanup() {
    runuser -u postgres -- psql -v ON_ERROR_STOP=1 postgres \
        -c "DROP DATABASE IF EXISTS $db" >/dev/null 2>&1 || true
    runuser -u postgres -- psql -v ON_ERROR_STOP=1 postgres \
        -c "DROP ROLE IF EXISTS $role" >/dev/null 2>&1 || true
    rm -rf "$tmp"
}
trap cleanup EXIT

runuser -u postgres -- psql -v ON_ERROR_STOP=1 postgres \
    -c "CREATE ROLE $role LOGIN PASSWORD '$password'" >/dev/null
runuser -u postgres -- psql -v ON_ERROR_STOP=1 postgres \
    -c "CREATE DATABASE $db OWNER $role" >/dev/null

pg_url="postgresql://${role}:${password}@127.0.0.1:5432/${db}"
PGPASSWORD="$password" psql -v ON_ERROR_STOP=1 -U "$role" -h 127.0.0.1 "$db" <<'SQL' >/dev/null
CREATE TABLE users (
    id integer PRIMARY KEY, username text, email text NOT NULL, name text,
    encrypted_password text NOT NULL, admin boolean NOT NULL, state text,
    user_type integer NOT NULL
);
CREATE TABLE namespaces (
    id integer PRIMARY KEY, name text NOT NULL, path text NOT NULL, type text NOT NULL,
    owner_id integer, parent_id integer
);
CREATE TABLE projects (
    id integer PRIMARY KEY, name text, path text NOT NULL, description text,
    namespace_id integer NOT NULL, visibility_level integer NOT NULL,
    archived boolean NOT NULL, lfs_enabled boolean, storage_version integer,
    pending_delete boolean, repository_storage text NOT NULL DEFAULT 'default',
    created_at timestamp NOT NULL, updated_at timestamp NOT NULL
);
CREATE TABLE members (
    source_type text NOT NULL, source_id integer NOT NULL, user_id integer,
    access_level integer NOT NULL, requested_at timestamp, invite_token text
);
CREATE TABLE keys (
    user_id integer, title text, key text, type text, fingerprint_sha256 bytea
);
CREATE TABLE lfs_objects (
    id integer PRIMARY KEY, oid text NOT NULL, size bigint NOT NULL, file_store integer NOT NULL
);
CREATE TABLE lfs_objects_projects (project_id integer NOT NULL, lfs_object_id integer NOT NULL);
CREATE TABLE fork_network_members (project_id integer NOT NULL, forked_from_project_id integer);

INSERT INTO users VALUES
    (1, 'alice', 'alice@example.test', 'Alice', 'invalid-test-hash', true, 'active', 0);
INSERT INTO namespaces VALUES
    (1, 'Alice', 'alice', 'User', 1, NULL),
    (10, 'Team', 'team', 'Group', NULL, NULL),
    (11, 'Platform', 'platform', 'Group', NULL, 10);
INSERT INTO projects VALUES
    (100, 'Demo', 'demo', 'demo project', 11, 0, false, true, 2, false, 'default',
     '2023-01-02 03:04:05', '2024-06-07 08:09:10'),
    (101, 'Fork', 'fork', 'fork project', 1, 0, false, true, 2, false, 'default',
     '2023-02-03 04:05:06', '2024-07-08 09:10:11');
INSERT INTO members VALUES ('Namespace', 10, 1, 40, NULL, NULL);
INSERT INTO keys VALUES (
    1, 'test key', 'ssh-ed25519 AAAAC3NzaC1lZDI1NTE5AAAAIEexample test', NULL,
    decode(repeat('01', 32), 'hex')
);
INSERT INTO fork_network_members VALUES (101, 100);
SQL

repos="$tmp/gitlab-repositories"
lfs="$tmp/gitlab-lfs"
out="$tmp/output"
shared_out="$tmp/shared-output"
work="$tmp/work"
mkdir -p "$repos" "$lfs" "$work"

create_repo_path() {
    local id="$1"
    local hash
    hash="$(printf '%s' "$id" | sha256sum | cut -d' ' -f1)"
    printf '%s/@hashed/%s/%s/%s.git' "$repos" "${hash:0:2}" "${hash:2:2}" "$hash"
}

source_repo="$(create_repo_path 100)"
fork_repo="$(create_repo_path 101)"
mkdir -p "$(dirname "$source_repo")" "$(dirname "$fork_repo")"
git init --bare --initial-branch=main "$source_repo" >/dev/null
git init --initial-branch=main "$work/source" >/dev/null
git -C "$work/source" config user.name 'Migration Test'
git -C "$work/source" config user.email 'migration@example.test'
printf '%s\n' 'migration content' >"$work/source/README.md"
git -C "$work/source" add README.md
git -C "$work/source" commit -m initial >/dev/null
git -C "$work/source" remote add origin "$source_repo"
git -C "$work/source" push origin main >/dev/null
git clone --bare --local "$source_repo" "$fork_repo" >/dev/null

lfs_content="$tmp/lfs-content"
printf '%s' 'git lfs migration content' >"$lfs_content"
lfs_oid="$(sha256sum "$lfs_content" | cut -d' ' -f1)"
lfs_size="$(stat -c '%s' "$lfs_content")"
mkdir -p "$lfs/${lfs_oid:0:2}/${lfs_oid:2:2}"
cp "$lfs_content" "$lfs/${lfs_oid:0:2}/${lfs_oid:2:2}/${lfs_oid:4}"
PGPASSWORD="$password" psql -v ON_ERROR_STOP=1 -U "$role" -h 127.0.0.1 "$db" \
    -v oid="$lfs_oid" -v size="$lfs_size" <<'SQL' >/dev/null
INSERT INTO lfs_objects VALUES (1, :'oid', :size, 1);
INSERT INTO lfs_objects_projects VALUES (100, 1);
SQL

cargo build -p rgit-migrate >/dev/null
"$repo_root/target/debug/rgit-migrate" \
    --pg "$pg_url" \
    --gitlab-repos "$repos" \
    --gitlab-lfs "$lfs" \
    --out "$out"

test "$(sqlite3 "$out/rgit.db" 'SELECT COUNT(*) FROM projects')" = "2"
test "$(sqlite3 "$out/rgit.db" "SELECT path FROM namespaces WHERE id=11")" = "team--platform"
test "$(sqlite3 "$out/rgit.db" "SELECT access_level FROM group_members WHERE namespace_id=11 AND user_id=1")" = "40"
test "$(sqlite3 "$out/rgit.db" "SELECT forked_from_project_id FROM projects WHERE id=101")" = "100"
test "$(sqlite3 "$out/rgit.db" "SELECT updated_at FROM projects WHERE id=100")" = "2024-06-07 08:09:10"
test "$(sqlite3 "$out/rgit.db" "SELECT COUNT(*) FROM project_lfs_objects WHERE project_id=100")" = "1"
test "$(sqlite3 "$out/rgit.db" "SELECT COUNT(*) FROM ssh_keys WHERE user_id=1")" = "1"

for id in 100 101; do
    hash="$(printf '%s' "$id" | sha256sum | cut -d' ' -f1)"
    git -C "$out/repositories/@hashed/${hash:0:2}/${hash:2:2}/${hash}.git" \
        fsck --full --no-progress >/dev/null
done
test "$(sha256sum "$out/lfs-objects/${lfs_oid:0:2}/${lfs_oid:2:2}/${lfs_oid:4}" | cut -d' ' -f1)" = "$lfs_oid"
test "$(sqlite3 "$out/rgit.db" 'PRAGMA integrity_check')" = "ok"

# Shared-storage mode must write metadata only and must not invoke git at all.
mkdir -p "$tmp/fake-bin"
printf '%s\n' '#!/usr/bin/env bash' 'echo "git must not run in --reuse-storage mode" >&2' 'exit 99' \
    >"$tmp/fake-bin/git"
chmod +x "$tmp/fake-bin/git"
PATH="$tmp/fake-bin:/usr/bin:/bin" "$repo_root/target/debug/rgit-migrate" \
    --pg "$pg_url" \
    --gitlab-repos "$repos" \
    --gitlab-lfs "$lfs" \
    --out "$shared_out" \
    --reuse-storage

test -f "$shared_out/rgit.db"
test -f "$shared_out/migration-report.json"
test ! -e "$shared_out/repositories"
test ! -e "$shared_out/lfs-objects"
test "$(sqlite3 "$shared_out/rgit.db" "SELECT default_branch FROM projects WHERE id=100")" = "main"
test "$(sqlite3 "$shared_out/rgit.db" "SELECT created_at FROM projects WHERE id=101")" = "2023-02-03 04:05:06"
test "$(python3 -c 'import json,sys; print(json.load(open(sys.argv[1]))["storage_mode"])' "$shared_out/migration-report.json")" = "reuse"

echo "GitLab migration integration test passed"
