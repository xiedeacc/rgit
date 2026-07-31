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

fake_root="$tmp/rgit"
mkdir -p "$fake_root/bin" "$fake_root/data" "$fake_root/logs" "$fake_root/.ssh" "$fake_root/.backup-worktree"
install -m 0755 "$repo_root/target/release/rgit" "$fake_root/bin/rgit"
install -m 0755 "$repo_root/scripts/rgit-backup.sh" "$fake_root/bin/rgit-backup"

grep -q "install or migrate rgit systemd units once on the NAS" "$repo_root/scripts/deploy.sh"
grep -q "deployment scripts must not generate long-lived systemd units" "$repo_root/scripts/deploy.sh"

echo "deployment package test passed"
