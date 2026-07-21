#!/usr/bin/env bash
set -euo pipefail

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
tmp="$(mktemp -d)"
trap 'rm -rf "$tmp"' EXIT

bash -n \
    "$repo_root/scripts/deploy.sh" \
    "$repo_root/scripts/rgit-backup.sh" \
    "$repo_root/scripts/rgit-restore.sh" \
    "$repo_root/scripts/rgit-refresh-ocsp.sh"

test -x "$repo_root/target/release/rgit"
test -x "$repo_root/target/release/rgit-shell"
test -x "$repo_root/target/release/rgit-migrate"
test -f "$repo_root/web/build/web/index.html"
grep -q 'canvasKitBaseUrl: "canvaskit/"' "$repo_root/web/build/web/flutter_bootstrap.js"

test -f "$repo_root/conf/rgit.example.toml"
test -f "$repo_root/scripts/systemd/rgit.service"
test -f "$repo_root/scripts/systemd/rgit-backup.service"
test -f "$repo_root/scripts/systemd/rgit-backup.timer"

fake_root="$tmp/rgit"
mkdir -p "$fake_root/bin" "$fake_root/data" "$fake_root/logs" "$fake_root/.ssh" "$fake_root/.backup-worktree"
install -m 0755 "$repo_root/target/release/rgit" "$fake_root/bin/rgit"
install -m 0755 "$repo_root/scripts/rgit-backup.sh" "$fake_root/bin/rgit-backup"

verify_units="$tmp/systemd"
mkdir -p "$verify_units"
sed "s#/opt/usr/local/rgit#$fake_root#g" \
    "$repo_root/scripts/systemd/rgit.service" >"$verify_units/rgit.service"
sed "s#/opt/usr/local/rgit#$fake_root#g" \
    "$repo_root/scripts/systemd/rgit-backup.service" >"$verify_units/rgit-backup.service"
cp "$repo_root/scripts/systemd/rgit-backup.timer" "$verify_units/rgit-backup.timer"

systemd-analyze verify \
    "$verify_units/rgit.service" \
    "$verify_units/rgit-backup.service" \
    "$verify_units/rgit-backup.timer"

grep -q "ExecStart=/opt/usr/local/rgit/bin/rgit" "$repo_root/scripts/systemd/rgit.service"
grep -q "/zfs/gitlab_data/repositories /zfs/gitlab_data/lfs-objects /var/opt/gitlab/.ssh" \
    "$repo_root/scripts/systemd/rgit.service"
grep -q "Environment=RGIT_BACKUP_ROOT=/opt/usr/local/rgit" \
    "$repo_root/scripts/systemd/rgit-backup.service"

echo "deployment package test passed"
