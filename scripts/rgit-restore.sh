#!/usr/bin/env bash
# Restore an rgit install from the backup repo (DESIGN.md §12):
# clone → reassemble split files → move into place → restore sqlite db.
set -euo pipefail

BACKUP_REPO_URL="${RGIT_BACKUP_REPO_URL:-git@github.com:xiedeacc/rgit_data.git}"
BACKUP_BRANCH="${RGIT_BACKUP_BRANCH:-master}"
TARGET_DIR="${1:-/opt/usr/local/rgit}"

log() { echo "[rgit-restore] $*"; }

if [ -e "$TARGET_DIR/data/rgit.db" ]; then
    log "refusing to overwrite existing $TARGET_DIR/data/rgit.db"
    exit 1
fi

tmp="$(mktemp -d)"
trap 'rm -rf "$tmp"' EXIT

log "cloning $BACKUP_REPO_URL"
git clone --depth 1 --branch "$BACKUP_BRANCH" "$BACKUP_REPO_URL" "$tmp/mirror"

log "reassembling split files"
find "$tmp/mirror" -name '*.rgit-split' -not -path '*/.git/*' | while read -r marker; do
    base="${marker%.rgit-split}"
    rel="$(grep '^original=' "$marker" | cut -d= -f2-)"
    log "  $rel"
    : >"$base"
    i=0
    while [ -f "$base.$i" ]; do
        cat "$base.$i" >>"$base"
        rm -f "$base.$i"
        i=$((i + 1))
    done
    rm -f "$marker"
done

log "copying into $TARGET_DIR"
mkdir -p "$TARGET_DIR"
rsync -a --exclude '/.git/' "$tmp/mirror/" "$TARGET_DIR/"

if [ -f "$TARGET_DIR/data/rgit.db.bak" ]; then
    mv "$TARGET_DIR/data/rgit.db.bak" "$TARGET_DIR/data/rgit.db"
    log "sqlite db restored from snapshot"
fi

log "done — review conf/rgit.toml, then start rgit.service"
