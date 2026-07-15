#!/usr/bin/env bash
# Deploy rgit to the NAS layout /opt/usr/local/rgit/{bin,conf,data,logs}
# and install systemd units (rgit.service, rgit-backup.service/.timer).
# Mirrors rblog's deploy.sh conventions. Run as root on the target host,
# or adapt the rsync targets for remote deploys.
set -euo pipefail

DEST_DIR="${RGIT_DEST_DIR:-/opt/usr/local/rgit}"
RUN_USER="${RGIT_RUN_USER:-git}"
BACKUP_REPO_URL="${RGIT_BACKUP_REPO_URL:-git@github.com:xiedeacc/rgit_data.git}"
FLUTTER_BIN="${FLUTTER_BIN:-/root/src/software/flutter/bin/flutter}"

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"

log() { echo "[deploy] $*"; }

build() {
    log "building rust binaries (release)"
    (cd "$repo_root" && cargo build --release --bin rgit --bin rgit-migrate)
    log "building flutter web"
    (cd "$repo_root/web" && "$FLUTTER_BIN" build web --release)
}

layout() {
    log "creating layout under $DEST_DIR"
    mkdir -p "$DEST_DIR"/{bin,conf,data,logs}
    install -m 0755 "$repo_root/target/release/rgit" "$DEST_DIR/bin/rgit"
    install -m 0755 "$repo_root/target/release/rgit-migrate" "$DEST_DIR/bin/rgit-migrate"
    install -m 0755 "$repo_root/scripts/rgit-backup.sh" "$DEST_DIR/bin/rgit-backup"
    install -m 0755 "$repo_root/scripts/rgit-restore.sh" "$DEST_DIR/bin/rgit-restore"
    rm -rf "$DEST_DIR/bin/web"
    cp -r "$repo_root/web/build/web" "$DEST_DIR/bin/web"

    if [ ! -f "$DEST_DIR/conf/rgit.toml" ]; then
        install -m 0600 "$repo_root/conf/rgit.example.toml" "$DEST_DIR/conf/rgit.toml"
        log "installed conf/rgit.toml from example — EDIT IT (external_url, paths)"
    fi

    id -u "$RUN_USER" >/dev/null 2>&1 || useradd --system --home "$DEST_DIR" --shell /usr/sbin/nologin "$RUN_USER"
    chown -R "$RUN_USER":"$RUN_USER" "$DEST_DIR/data" "$DEST_DIR/logs" "$DEST_DIR/conf"
}

write_service() {
    cat >/etc/systemd/system/rgit.service <<EOF
[Unit]
Description=rgit git service
After=network.target

[Service]
Type=simple
User=${RUN_USER}
WorkingDirectory=${DEST_DIR}
Environment=RGIT_CONFIG=${DEST_DIR}/conf/rgit.toml
ExecStart=${DEST_DIR}/bin/rgit
Restart=always
RestartSec=3
StandardOutput=append:${DEST_DIR}/logs/rgit.log
StandardError=append:${DEST_DIR}/logs/rgit.log
# Hardening
NoNewPrivileges=true
ProtectSystem=strict
ReadWritePaths=${DEST_DIR}/data ${DEST_DIR}/logs
ProtectHome=true
PrivateTmp=true

[Install]
WantedBy=multi-user.target
EOF
}

write_backup_service() {
    cat >/etc/systemd/system/rgit-backup.service <<EOF
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
ExecStart=${DEST_DIR}/bin/rgit-backup
EOF
}

write_backup_timer() {
    cat >/etc/systemd/system/rgit-backup.timer <<EOF
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
    systemctl daemon-reload
    systemctl enable --now rgit.service
    systemctl enable --now rgit-backup.timer
    log "services enabled"
}

main() {
    build
    layout
    write_service
    write_backup_service
    write_backup_timer
    enable_units
    log "deployed. next: configure nginx (conf/nginx/rgit.conf) and check logs/rgit.log"
}

main "$@"
