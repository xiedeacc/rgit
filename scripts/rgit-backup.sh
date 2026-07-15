#!/usr/bin/env bash
# rgit backup — mirrors the install root to a GitHub repo (DESIGN.md §12).
# Ported from rblog's rblog-backup.sh with one addition: a consistent SQLite
# snapshot replaces the hot database file in the mirror.
set -euo pipefail

BACKUP_REPO_URL="${RGIT_BACKUP_REPO_URL:-git@github.com:xiedeacc/rgit_data.git}"
BACKUP_BRANCH="${RGIT_BACKUP_BRANCH:-master}"
MAX_FILE_BYTES="${RGIT_BACKUP_MAX_FILE_BYTES:-52428800}"   # 50 MiB commit threshold
SPLIT_BYTES="${RGIT_BACKUP_SPLIT_BYTES:-49000000}"          # ~49 MB chunk size

script_dir="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
install_dir="${RGIT_BACKUP_ROOT:-$(dirname "$script_dir")}"
work_dir="${RGIT_BACKUP_WORK_DIR:-${install_dir}/.backup-worktree}"
db_file="${RGIT_BACKUP_DB:-${install_dir}/data/rgit.db}"

log() {
    echo "[rgit-backup] $*"
}

require_command() {
    if ! command -v "$1" >/dev/null 2>&1; then
        log "required command not found: $1"
        exit 127
    fi
}

checkout_backup_branch() {
    if git -C "$work_dir" rev-parse --verify "origin/${BACKUP_BRANCH}" >/dev/null 2>&1; then
        git -C "$work_dir" checkout -B "$BACKUP_BRANCH" "origin/${BACKUP_BRANCH}"
    elif git -C "$work_dir" rev-parse --verify "$BACKUP_BRANCH" >/dev/null 2>&1; then
        git -C "$work_dir" checkout "$BACKUP_BRANCH"
    else
        git -C "$work_dir" checkout --orphan "$BACKUP_BRANCH"
    fi
}

ensure_repo() {
    if [ -d "${work_dir}/.git" ]; then
        git -C "$work_dir" remote set-url origin "$BACKUP_REPO_URL"
        git -C "$work_dir" fetch origin "$BACKUP_BRANCH" || true
        checkout_backup_branch
        git -C "$work_dir" pull --ff-only origin "$BACKUP_BRANCH" || true
        return
    fi
    if git clone --branch "$BACKUP_BRANCH" "$BACKUP_REPO_URL" "$work_dir" 2>/dev/null; then
        return
    fi
    log "clone failed (empty remote?) — initializing local repo"
    mkdir -p "$work_dir"
    git -C "$work_dir" init -b "$BACKUP_BRANCH"
    git -C "$work_dir" remote add origin "$BACKUP_REPO_URL"
}

# Consistent snapshot of the live SQLite DB (WAL-safe). The mirror carries
# data/rgit.db.bak instead of the hot db/-wal/-shm files.
snapshot_sqlite() {
    if [ -f "$db_file" ]; then
        rm -f "${db_file}.bak"
        sqlite3 "$db_file" "VACUUM INTO '${db_file}.bak'"
        log "sqlite snapshot written: ${db_file}.bak"
    else
        log "no sqlite db at ${db_file} — skipping snapshot"
    fi
}

sync_source() {
    rsync -a --delete \
        --exclude '/logs/' \
        --exclude "/$(basename "$work_dir")/" \
        --exclude '/.git/' \
        --exclude '/data/rgit.db' \
        --exclude '/data/rgit.db-wal' \
        --exclude '/data/rgit.db-shm' \
        "$install_dir/" "$work_dir/"
}

split_file() {
    local file="$1"
    python3 - "$file" "$SPLIT_BYTES" <<'PYEOF'
import sys

path, chunk = sys.argv[1], int(sys.argv[2])
i = 0
with open(path, "rb") as f:
    while True:
        data = f.read(chunk)
        if not data:
            break
        with open(f"{path}.{i}", "wb") as out:
            out.write(data)
        i += 1
PYEOF
}

ignore_path() {
    local rel="$1"
    touch "$work_dir/.gitignore"
    if ! grep -qxF "$rel" "$work_dir/.gitignore"; then
        echo "$rel" >>"$work_dir/.gitignore"
    fi
}

# Re-run state: drop previous chunks so fresh originals get re-split.
reset_generated_split_files() {
    find "$work_dir" -name '*.rgit-split' -not -path "*/.git/*" | while read -r marker; do
        local_base="${marker%.rgit-split}"
        rm -f "$local_base" "$local_base".[0-9]* "$marker"
    done
}

split_large_files() {
    find "$work_dir" -type f -size +"$MAX_FILE_BYTES"c \
        -not -path "*/.git/*" \
        -not -name '.gitignore' \
        -not -name '*.rgit-split' | while read -r file; do
        rel="${file#"$work_dir"/}"
        case "$rel" in
            *.[0-9]|*.[0-9][0-9]) continue ;;
        esac
        log "splitting large file: $rel"
        ignore_path "$rel"
        split_file "$file"
        {
            echo "original=$rel"
            echo "split_bytes=$SPLIT_BYTES"
        } >"${file}.rgit-split"
        rm -f "$file"
    done
}

commit_and_push_if_changed() {
    git -C "$work_dir" add -A
    if git -C "$work_dir" diff --cached --quiet; then
        log "no changes to back up"
        # Re-push in case a previous push failed after commit.
        if git -C "$work_dir" rev-parse --verify HEAD >/dev/null 2>&1; then
            git -C "$work_dir" push origin "$BACKUP_BRANCH" || true
        fi
        return
    fi
    if [ -z "$(git -C "$work_dir" config user.email || true)" ]; then
        git -C "$work_dir" config user.email "rgit-backup@localhost"
        git -C "$work_dir" config user.name "rgit backup"
    fi
    git -C "$work_dir" commit -m "Backup $(date -u +'%Y-%m-%dT%H:%M:%SZ')"
    git -C "$work_dir" push origin "$BACKUP_BRANCH"
    log "backup pushed"
}

main() {
    require_command git
    require_command rsync
    require_command python3
    require_command sqlite3
    require_command find

    if [ ! -d "$install_dir" ]; then
        log "install dir not found: $install_dir"
        exit 1
    fi

    log "backing up $install_dir -> $BACKUP_REPO_URL ($BACKUP_BRANCH)"
    ensure_repo
    reset_generated_split_files
    snapshot_sqlite
    sync_source
    split_large_files
    commit_and_push_if_changed
    log "done"
}

main "$@"
