#!/usr/bin/env bash
# Build rgit on the dev machine, upload release artifacts to the NAS, install
# them into /opt/usr/local/rgit, upload systemd units, restart, and verify.
set -euo pipefail

REMOTE_HOST="${RGIT_DEPLOY_HOST:-nas}"
DEST_DIR="${RGIT_DEST_DIR:-/opt/usr/local/rgit}"
REMOTE_TMP_ROOT="${RGIT_REMOTE_TMP_ROOT:-/tmp}"
RUN_USER="${RGIT_RUN_USER:-git}"
SYSTEMD_DIR="${RGIT_SYSTEMD_DIR:-/etc/systemd/system}"
FLUTTER_BIN="${FLUTTER_BIN:-/root/src/software/flutter/bin/flutter}"
SKIP_BUILD="${RGIT_SKIP_BUILD:-0}"
ENABLE_UNITS="${RGIT_ENABLE_UNITS:-1}"
RESTART_SERVICE="${RGIT_RESTART_SERVICE:-1}"
START_BACKUP_TIMER="${RGIT_START_BACKUP_TIMER:-1}"
VERIFY_DEPLOY="${RGIT_VERIFY_DEPLOY:-1}"
VERIFY_URL="${RGIT_VERIFY_URL:-https://rgit.xiedeacc.com}"

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"

log() { echo "[deploy] $*"; }
die() {
    echo "[deploy] error: $*" >&2
    exit 1
}

build_rev() {
    git -C "$repo_root" rev-parse --short=8 HEAD
}

build_time() {
    TZ=Asia/Shanghai git -C "$repo_root" show -s --format=%cd --date=format-local:'%Y%m%d %H:%M' HEAD
}

require_default_dest() {
    if [ "$DEST_DIR" != "/opt/usr/local/rgit" ]; then
        die "RGIT_DEST_DIR=$DEST_DIR is unsupported because scripts/systemd units are copied verbatim from NAS and point to /opt/usr/local/rgit"
    fi
}

build_artifacts() {
    if [ "$SKIP_BUILD" = "1" ]; then
        log "using existing release and Flutter build artifacts"
        return
    fi

    log "step 1/6: building Rust release binaries on dev"
    (cd "$repo_root" && cargo build --release --bin rgit --bin rgit-shell --bin rgit-migrate)

    log "step 2/6: building Flutter Web on dev"
    local rev time
    rev="$(build_rev)"
    time="$(build_time)"
    (cd "$repo_root/web" && "$FLUTTER_BIN" build web --release \
        --dart-define="RGIT_BUILD_REV=${rev}" \
        --dart-define="RGIT_BUILD_TIME=${time}")
}

check_artifacts() {
    test -x "$repo_root/target/release/rgit" || die "missing target/release/rgit"
    test -x "$repo_root/target/release/rgit-shell" || die "missing target/release/rgit-shell"
    test -x "$repo_root/target/release/rgit-migrate" || die "missing target/release/rgit-migrate"
    test -f "$repo_root/web/build/web/index.html" || die "missing web/build/web/index.html"
    test -f "$repo_root/conf/rgit.example.toml" || die "missing conf/rgit.example.toml"
    test -f "$repo_root/scripts/systemd/rgit.service" || die "missing scripts/systemd/rgit.service"
    test -f "$repo_root/scripts/systemd/rgit-backup.service" || die "missing scripts/systemd/rgit-backup.service"
    test -f "$repo_root/scripts/systemd/rgit-backup.timer" || die "missing scripts/systemd/rgit-backup.timer"
}

prepare_remote() {
    local remote_dir="$1"
    log "step 3/6: preparing NAS staging directory $REMOTE_HOST:$remote_dir"
    ssh "$REMOTE_HOST" bash -s -- "$remote_dir" <<'REMOTE'
set -euo pipefail
remote_dir="$1"
rm -rf "$remote_dir"
mkdir -p "$remote_dir/bin" "$remote_dir/scripts" "$remote_dir/web" "$remote_dir/conf" "$remote_dir/systemd"
REMOTE
}

upload_artifacts() {
    local remote_dir="$1"
    log "step 4/6: uploading release artifacts to NAS"
    rsync -a \
        "$repo_root/target/release/rgit" \
        "$repo_root/target/release/rgit-shell" \
        "$repo_root/target/release/rgit-migrate" \
        "$REMOTE_HOST:$remote_dir/bin/"
    rsync -a \
        "$repo_root/scripts/rgit-backup.sh" \
        "$repo_root/scripts/rgit-restore.sh" \
        "$repo_root/scripts/rgit-refresh-ocsp.sh" \
        "$REMOTE_HOST:$remote_dir/scripts/"
    rsync -a "$repo_root/conf/rgit.example.toml" "$REMOTE_HOST:$remote_dir/conf/"
    rsync -a \
        "$repo_root/scripts/systemd/rgit.service" \
        "$repo_root/scripts/systemd/rgit-backup.service" \
        "$repo_root/scripts/systemd/rgit-backup.timer" \
        "$REMOTE_HOST:$remote_dir/systemd/"
    rsync -a --delete "$repo_root/web/build/web/" "$REMOTE_HOST:$remote_dir/web/"
}

install_remote() {
    local remote_dir="$1"
    log "step 5/6: installing artifacts and systemd units on NAS"
    ssh "$REMOTE_HOST" bash -s -- \
        "$remote_dir" "$DEST_DIR" "$SYSTEMD_DIR" "$RUN_USER" \
        "$ENABLE_UNITS" "$RESTART_SERVICE" "$START_BACKUP_TIMER" <<'REMOTE'
set -euo pipefail
src="$1"
dest="$2"
systemd_dir="$3"
run_user="$4"
enable_units="$5"
restart_service="$6"
start_backup_timer="$7"

mkdir -p "$dest/bin" "$dest/conf" "$dest/data" "$dest/logs"
install -m 0755 "$src/bin/rgit" "$dest/bin/rgit"
install -m 0755 "$src/bin/rgit-shell" "$dest/bin/rgit-shell"
install -m 0755 "$src/bin/rgit-migrate" "$dest/bin/rgit-migrate"
install -m 0755 "$src/scripts/rgit-backup.sh" "$dest/bin/rgit-backup"
install -m 0755 "$src/scripts/rgit-restore.sh" "$dest/bin/rgit-restore"
install -m 0755 "$src/scripts/rgit-refresh-ocsp.sh" "$dest/bin/rgit-refresh-ocsp"

rm -rf "$dest/bin/web.new" "$dest/bin/web.old"
cp -a "$src/web" "$dest/bin/web.new"
chmod -R u=rwX,go=rX "$dest/bin/web.new"
if [ -d "$dest/bin/web" ]; then
    mv "$dest/bin/web" "$dest/bin/web.old"
fi
mv "$dest/bin/web.new" "$dest/bin/web"

if [ ! -f "$dest/conf/rgit.toml" ]; then
    install -m 0600 "$src/conf/rgit.example.toml" "$dest/conf/rgit.toml"
    echo "[deploy] installed conf/rgit.toml from example; edit it before first start"
fi

id -u "$run_user" >/dev/null 2>&1 || useradd --system --home "$dest" --shell /usr/sbin/nologin "$run_user"
install -d -m 0700 -o "$run_user" -g "$run_user" "$dest/.ssh"
install -d -m 0700 -o "$run_user" -g "$run_user" "$dest/.backup-worktree"
chown -R "$run_user":"$run_user" "$dest/data" "$dest/logs" "$dest/conf"

mkdir -p "$systemd_dir"
install -m 0644 "$src/systemd/rgit.service" "$systemd_dir/rgit.service"
install -m 0644 "$src/systemd/rgit-backup.service" "$systemd_dir/rgit-backup.service"
install -m 0644 "$src/systemd/rgit-backup.timer" "$systemd_dir/rgit-backup.timer"

systemctl daemon-reload
if [ "$enable_units" = "1" ]; then
    systemctl enable rgit.service rgit-backup.timer
fi
if [ "$restart_service" = "1" ]; then
    systemctl restart rgit.service
fi
if [ "$start_backup_timer" = "1" ]; then
    systemctl start rgit-backup.timer
fi
systemctl is-active rgit.service
REMOTE
}

verify_remote() {
    local rev="$1"
    if [ "$VERIFY_DEPLOY" != "1" ]; then
        log "step 6/6: verification skipped"
        return
    fi

    log "step 6/6: verifying deployed service and Web bundle"
    local bundle
    bundle="$(mktemp)"
    local found=0
    for _ in $(seq 1 20); do
        curl -sk -H 'Cache-Control: no-cache' "$VERIFY_URL/main.dart.js?rev=$rev" -o "$bundle"
        if grep -q "$rev" "$bundle"; then
            found=1
            break
        fi
        sleep 1
    done
    rm -f "$bundle"
    [ "$found" = "1" ] || die "deployed Web bundle does not contain $rev"
    ssh "$REMOTE_HOST" bash -s <<'REMOTE'
set -euo pipefail
test "$(systemctl is-active rgit.service)" = "active"
test "$(systemctl is-active gitlab-runsvdir.service || true)" = "inactive"
REMOTE
}

main() {
    require_default_dest
    build_artifacts
    check_artifacts

    local rev remote_dir
    rev="$(build_rev)"
    remote_dir="$REMOTE_TMP_ROOT/rgit-deploy-$rev"

    prepare_remote "$remote_dir"
    upload_artifacts "$remote_dir"
    install_remote "$remote_dir"
    verify_remote "$rev"
    log "deployed $rev to $REMOTE_HOST:$DEST_DIR"
}

main "$@"
