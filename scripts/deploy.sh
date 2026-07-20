#!/usr/bin/env bash
# Deploy rgit to the NAS layout /opt/usr/local/rgit/{bin,conf,data,logs}
# and install systemd units (rgit.service, rgit-backup.service/.timer).
# Mirrors rblog's deploy.sh conventions. Run as root on the target host.
set -euo pipefail

DEST_DIR="${RGIT_DEST_DIR:-/opt/usr/local/rgit}"
RUN_USER="${RGIT_RUN_USER:-git}"
BACKUP_REPO_URL="${RGIT_BACKUP_REPO_URL:-git@github.com:xiedeacc/rgit_data.git}"
FLUTTER_BIN="${FLUTTER_BIN:-/root/src/software/flutter/bin/flutter}"
START_SERVICES="${RGIT_START_SERVICES:-0}"
SKIP_BUILD="${RGIT_SKIP_BUILD:-0}"
SYSTEMD_DIR="${RGIT_SYSTEMD_DIR:-/etc/systemd/system}"
ENABLE_UNITS="${RGIT_ENABLE_UNITS:-1}"
REPOSITORIES_DIR="${RGIT_REPOSITORIES_DIR:-/zfs/gitlab_data/repositories}"
LFS_OBJECTS_DIR="${RGIT_LFS_OBJECTS_DIR:-/zfs/gitlab_data/lfs-objects}"
AUTHORIZED_KEYS_DIR="${RGIT_AUTHORIZED_KEYS_DIR:-/var/opt/gitlab/.ssh}"

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"

log() { echo "[deploy] $*"; }

build_rev() {
    git -C "$repo_root" rev-parse --short=8 HEAD
}

build_time() {
    TZ=Asia/Shanghai git -C "$repo_root" show -s --format=%cd --date=format-local:'%Y%m%d %H:%M' HEAD
}

build() {
    log "building rust binaries (release)"
    (cd "$repo_root" && cargo build --release --bin rgit --bin rgit-shell --bin rgit-migrate)
    log "building flutter web"
    local rev time
    rev="$(build_rev)"
    time="$(build_time)"
    (cd "$repo_root/web" && "$FLUTTER_BIN" build web --release \
        --dart-define="RGIT_BUILD_REV=${rev}" \
        --dart-define="RGIT_BUILD_TIME=${time}")
}

layout() {
    log "creating layout under $DEST_DIR"
    mkdir -p "$DEST_DIR"/{bin,conf,data,logs}
    install -m 0755 "$repo_root/target/release/rgit" "$DEST_DIR/bin/rgit"
    install -m 0755 "$repo_root/target/release/rgit-shell" "$DEST_DIR/bin/rgit-shell"
    install -m 0755 "$repo_root/target/release/rgit-migrate" "$DEST_DIR/bin/rgit-migrate"
    install -m 0755 "$repo_root/scripts/rgit-backup.sh" "$DEST_DIR/bin/rgit-backup"
    install -m 0755 "$repo_root/scripts/rgit-restore.sh" "$DEST_DIR/bin/rgit-restore"
    install -m 0755 "$repo_root/scripts/rgit-refresh-ocsp.sh" "$DEST_DIR/bin/rgit-refresh-ocsp"
    rm -rf "$DEST_DIR/bin/web"
    cp -r "$repo_root/web/build/web" "$DEST_DIR/bin/web"
    chmod -R u=rwX,go=rX "$DEST_DIR/bin/web"

    if [ ! -f "$DEST_DIR/conf/rgit.toml" ]; then
        install -m 0600 "$repo_root/conf/rgit.example.toml" "$DEST_DIR/conf/rgit.toml"
        log "installed conf/rgit.toml from example — EDIT IT (external_url, paths)"
    fi

    id -u "$RUN_USER" >/dev/null 2>&1 || useradd --system --home "$DEST_DIR" --shell /usr/sbin/nologin "$RUN_USER"
    install -d -m 0700 -o "$RUN_USER" -g "$RUN_USER" "$DEST_DIR/.ssh"
    install -d -m 0700 -o "$RUN_USER" -g "$RUN_USER" "$DEST_DIR/.backup-worktree"
    chown -R "$RUN_USER":"$RUN_USER" "$DEST_DIR/data" "$DEST_DIR/logs" "$DEST_DIR/conf"
}

write_service() {
    mkdir -p "$SYSTEMD_DIR"
    cat >"$SYSTEMD_DIR/rgit.service" <<EOF
[Unit]
Description=rgit git service
After=network.target

[Service]
Type=simple
User=${RUN_USER}
WorkingDirectory=${DEST_DIR}
Environment=RGIT_CONFIG=${DEST_DIR}/conf/rgit.toml
Environment=HOME=${DEST_DIR}
ExecStart=${DEST_DIR}/bin/rgit
Restart=always
RestartSec=3
TimeoutStopSec=45
StandardOutput=append:${DEST_DIR}/logs/rgit.log
StandardError=append:${DEST_DIR}/logs/rgit.log
# Hardening
NoNewPrivileges=true
ProtectSystem=strict
ReadWritePaths=${DEST_DIR}/data ${DEST_DIR}/logs ${REPOSITORIES_DIR} ${LFS_OBJECTS_DIR} ${AUTHORIZED_KEYS_DIR}
ProtectHome=true
PrivateTmp=true
PrivateDevices=true
ProtectKernelTunables=true
ProtectKernelModules=true
ProtectKernelLogs=true
ProtectControlGroups=true
RestrictSUIDSGID=true
LockPersonality=true
UMask=0077
LimitNOFILE=65536

[Install]
WantedBy=multi-user.target
EOF
}

write_backup_service() {
    cat >"$SYSTEMD_DIR/rgit-backup.service" <<EOF
[Unit]
Description=rgit backup to GitHub
After=network-online.target

[Service]
Type=oneshot
User=${RUN_USER}
WorkingDirectory=${DEST_DIR}
Environment=RGIT_BACKUP_REPO_URL=${BACKUP_REPO_URL}
Environment=RGIT_BACKUP_ROOT=${DEST_DIR}
Environment=RGIT_BACKUP_WORK_DIR=${DEST_DIR}/.backup-worktree
Environment=HOME=${DEST_DIR}
Environment="GIT_SSH_COMMAND=ssh -i ${DEST_DIR}/.ssh/id_ed25519 -o IdentitiesOnly=yes -o UserKnownHostsFile=${DEST_DIR}/.ssh/known_hosts"
ExecStart=${DEST_DIR}/bin/rgit-backup
NoNewPrivileges=true
ProtectSystem=strict
ReadWritePaths=${DEST_DIR}
ProtectHome=true
PrivateTmp=true
PrivateDevices=true
ProtectKernelTunables=true
ProtectKernelModules=true
ProtectKernelLogs=true
ProtectControlGroups=true
RestrictSUIDSGID=true
LockPersonality=true
UMask=0077
EOF
}

write_backup_timer() {
    cat >"$SYSTEMD_DIR/rgit-backup.timer" <<EOF
[Unit]
Description=hourly rgit backup

[Timer]
OnBootSec=5min
OnUnitActiveSec=1h
AccuracySec=1min
Persistent=true
Unit=rgit-backup.service

[Install]
WantedBy=timers.target
EOF
}

enable_units() {
    if [ "$ENABLE_UNITS" != "1" ]; then
        log "systemd enable/start skipped"
        return
    fi
    systemctl daemon-reload
    systemctl enable rgit.service rgit-backup.timer
    if [ "$START_SERVICES" = "1" ]; then
        systemctl start rgit.service rgit-backup.timer
        log "services enabled and started"
    else
        log "services enabled but not started; configure or migrate data first"
    fi
}

main() {
    if [ "$SKIP_BUILD" = "1" ]; then
        log "using existing release and Flutter build artifacts"
    else
        build
    fi
    layout
    write_service
    write_backup_service
    write_backup_timer
    enable_units
    log "deployed. next: edit conf/rgit.toml, migrate if needed, then start rgit.service"
}

main "$@"
