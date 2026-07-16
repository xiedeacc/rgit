#!/usr/bin/env bash
set -euo pipefail

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
tmp="$(mktemp -d)"
trap 'rm -rf "$tmp"' EXIT

install="$tmp/install"
remote="$tmp/backup.git"
restore="$tmp/restore"
mkdir -p "$install/data" "$install/conf" "$install/.ssh"
git init --bare "$remote" >/dev/null
sqlite3 "$install/data/rgit.db" \
    "CREATE TABLE sample (value TEXT NOT NULL);
     CREATE TABLE projects (id INTEGER PRIMARY KEY, disk_hash TEXT NOT NULL);
     INSERT INTO sample VALUES ('first');
     INSERT INTO projects VALUES (1, 'hash');"
printf '%s\n' 'migration metadata' >"$install/data/migration-report.json"
printf '%s\n' 'test-config' >"$install/conf/rgit.toml"
mkdir -p "$install/bin" "$install/logs"
printf '%s\n' 'test-binary' >"$install/bin/rgit"
printf '%s\n' 'must-not-be-backed-up' >"$install/logs/rgit.log"
printf '%s\n' 'must-not-be-backed-up' >"$install/.ssh/id_ed25519"
dd if=/dev/zero of="$install/data/large.bin" bs=4096 count=4 status=none

run_backup() {
    RGIT_BACKUP_REPO_URL="$remote" \
    RGIT_BACKUP_ROOT="$install" \
    RGIT_BACKUP_WORK_DIR="$install/.backup-worktree" \
    RGIT_BACKUP_MAX_FILE_BYTES=1024 \
    RGIT_BACKUP_SPLIT_BYTES=900 \
        "$repo_root/scripts/rgit-backup.sh"
}

run_backup
first_commit="$(git --git-dir="$remote" rev-list --count master)"
if git --git-dir="$remote" ls-tree -r --name-only master | grep -q '^\.ssh/'; then
    echo "SSH credentials were included in the backup" >&2
    exit 1
fi
if git --git-dir="$remote" ls-tree -r --name-only master | grep -q '^data/\.backup\.lock$'; then
    echo "backup lock was included in the backup" >&2
    exit 1
fi
if git --git-dir="$remote" ls-tree -r --name-only master | grep -Eq '^data/(repositories|lfs-objects)/'; then
    echo "repository or LFS data was included in the backup" >&2
    exit 1
fi
if ! git --git-dir="$remote" ls-tree -r --name-only master | grep -q '^bin/rgit$'; then
    echo "bin/ was not included in the backup" >&2
    exit 1
fi
if ! git --git-dir="$remote" ls-tree -r --name-only master | grep -q '^conf/rgit.toml$'; then
    echo "conf/ was not included in the backup" >&2
    exit 1
fi
if git --git-dir="$remote" ls-tree -r --name-only master | grep -q '^logs/'; then
    echo "logs/ was included in the backup" >&2
    exit 1
fi
run_backup
second_commit="$(git --git-dir="$remote" rev-list --count master)"
if [ "$first_commit" != "$second_commit" ]; then
    echo "unchanged backup unexpectedly created a commit" >&2
    exit 1
fi

sqlite3 "$install/data/rgit.db" "INSERT INTO sample VALUES ('second');"
run_backup
if [ "$(git --git-dir="$remote" rev-list --count master)" -ne $((second_commit + 1)) ]; then
    echo "changed backup did not create a commit" >&2
    exit 1
fi

RGIT_BACKUP_REPO_URL="$remote" "$repo_root/scripts/rgit-restore.sh" "$restore"
if [ "$(sqlite3 "$restore/data/rgit.db" 'SELECT COUNT(*) FROM sample')" != "2" ]; then
    echo "restored sqlite data does not match" >&2
    exit 1
fi
cmp "$install/data/large.bin" "$restore/data/large.bin"
cmp "$install/data/migration-report.json" "$restore/data/migration-report.json"
cmp "$install/bin/rgit" "$restore/bin/rgit"
cmp "$install/conf/rgit.toml" "$restore/conf/rgit.toml"
[ ! -e "$restore/.ssh/id_ed25519" ]
[ ! -e "$restore/logs/rgit.log" ]

echo "backup/restore integration test passed"
