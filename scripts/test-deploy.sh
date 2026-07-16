#!/usr/bin/env bash
set -euo pipefail

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
tmp="$(mktemp -d)"
trap 'rm -rf "$tmp"' EXIT

test -x "$repo_root/target/release/rgit"
test -x "$repo_root/target/release/rgit-shell"
test -x "$repo_root/target/release/rgit-migrate"
test -f "$repo_root/web/build/web/index.html"
grep -q 'canvasKitBaseUrl: "canvaskit/"' "$repo_root/web/build/web/flutter_bootstrap.js"

RGIT_DEST_DIR="$tmp/install" \
RGIT_RUN_USER="$(id -un)" \
RGIT_SKIP_BUILD=1 \
RGIT_SYSTEMD_DIR="$tmp/systemd" \
RGIT_ENABLE_UNITS=0 \
    "$repo_root/scripts/deploy.sh"

test -x "$tmp/install/bin/rgit"
test -x "$tmp/install/bin/rgit-shell"
test -x "$tmp/install/bin/rgit-migrate"
test -x "$tmp/install/bin/rgit-backup"
test -x "$tmp/install/bin/rgit-restore"
test -f "$tmp/install/bin/web/index.html"
test "$(stat -c '%a' "$tmp/install/conf/rgit.toml")" = "600"
test "$(stat -c '%a' "$tmp/install/.ssh")" = "700"
test "$(stat -c '%a' "$tmp/install/.backup-worktree")" = "700"

systemd-analyze verify \
    "$tmp/systemd/rgit.service" \
    "$tmp/systemd/rgit-backup.service" \
    "$tmp/systemd/rgit-backup.timer"

grep -q "ReadWritePaths=$tmp/install/data $tmp/install/logs" "$tmp/systemd/rgit.service"
grep -q "/zfs/gitlab_data/repositories /zfs/gitlab_data/lfs-objects /var/opt/gitlab/.ssh" "$tmp/systemd/rgit.service"
grep -q "ReadWritePaths=$tmp/install" "$tmp/systemd/rgit-backup.service"
grep -q "Environment=HOME=$tmp/install" "$tmp/systemd/rgit.service"
grep -q "Environment=HOME=$tmp/install" "$tmp/systemd/rgit-backup.service"

echo "deployment integration test passed"
