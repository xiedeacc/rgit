#!/usr/bin/env bash
# Restore rgit's bin/, conf/, and data/ directories from the backup repository.
set -euo pipefail

BACKUP_REPO_URL="${RGIT_BACKUP_REPO_URL:-git@github.com:xiedeacc/rgit_data.git}"
BACKUP_BRANCH="${RGIT_BACKUP_BRANCH:-master}"
TARGET_DIR="${1:-/opt/usr/local/rgit}"

log() { echo "[rgit-restore] $*"; }

require_command() {
    if ! command -v "$1" >/dev/null 2>&1; then
        log "required command not found: $1"
        exit 127
    fi
}

for command in git sha256sum stat sqlite3 find; do
    require_command "$command"
done

if [ -e "$TARGET_DIR/data/rgit.db" ]; then
    log "refusing to overwrite existing $TARGET_DIR/data/rgit.db"
    exit 1
fi

tmp="$(mktemp -d)"
trap 'rm -rf "$tmp"' EXIT

log "cloning $BACKUP_REPO_URL"
git clone --depth 1 --branch "$BACKUP_BRANCH" "$BACKUP_REPO_URL" "$tmp/mirror"

if [ -f "$tmp/mirror/.rgit-empty-dirs" ]; then
    log "recreating empty directories"
    while read -r rel; do
        [ -z "$rel" ] && continue
        case "$rel" in
            /*|..|../*|*/../*|*/..) log "unsafe empty-directory path: $rel"; exit 1 ;;
        esac
        mkdir -p "$tmp/mirror/$rel"
    done <"$tmp/mirror/.rgit-empty-dirs"
    rm -f "$tmp/mirror/.rgit-empty-dirs"
fi

log "reassembling split files"
find "$tmp/mirror" -name '*.rgit-split' -not -path '*/.git/*' | while read -r marker; do
    base="${marker%.rgit-split}"
    rel="$(grep '^original=' "$marker" | cut -d= -f2-)"
    expected_size="$(grep '^size=' "$marker" | cut -d= -f2- || true)"
    expected_sha="$(grep '^sha256=' "$marker" | cut -d= -f2- || true)"
    expected_chunks="$(grep '^chunks=' "$marker" | cut -d= -f2- || true)"
    log "  $rel"
    : >"$base"
    i=0
    if [ -n "$expected_chunks" ]; then
        while [ "$i" -lt "$expected_chunks" ]; do
            if [ ! -f "$base.$i" ]; then
                log "missing split chunk: $rel.$i"
                exit 1
            fi
            cat "$base.$i" >>"$base"
            rm -f "$base.$i"
            i=$((i + 1))
        done
    else
        log "warning: legacy split marker has no checksum metadata: $rel"
        while [ -f "$base.$i" ]; do
            cat "$base.$i" >>"$base"
            rm -f "$base.$i"
            i=$((i + 1))
        done
        if [ "$i" -eq 0 ]; then
            log "no split chunks found: $rel"
            exit 1
        fi
    fi
    if [ -n "$expected_size" ] && [ "$(stat -c '%s' "$base")" != "$expected_size" ]; then
        log "restored size mismatch: $rel"
        exit 1
    fi
    if [ -n "$expected_sha" ] && [ "$(sha256sum "$base" | cut -d' ' -f1)" != "$expected_sha" ]; then
        log "restored sha256 mismatch: $rel"
        exit 1
    fi
    rm -f "$marker"
done

for directory in bin conf data; do
    if [ ! -d "$tmp/mirror/$directory" ]; then
        log "backup does not contain $directory/"
        exit 1
    fi
done

log "copying bin/, conf/, and data/ into $TARGET_DIR"
mkdir -p "$TARGET_DIR"
find "$tmp/mirror" -mindepth 1 -maxdepth 1 ! -name '.git' -exec cp -a --target-directory="$TARGET_DIR" -- {} +

if [ -f "$TARGET_DIR/data/rgit.db" ]; then
    integrity="$(sqlite3 "$TARGET_DIR/data/rgit.db" 'PRAGMA integrity_check')"
    if [ "$integrity" != "ok" ]; then
        log "sqlite integrity check failed: $integrity"
        exit 1
    fi

    sqlite3 "$TARGET_DIR/data/rgit.db" 'SELECT 1 FROM projects LIMIT 1' >/dev/null
fi

chmod 0600 "$TARGET_DIR/data/rgit.db" 2>/dev/null || true

log "done — repository and LFS storage are intentionally not restored"
