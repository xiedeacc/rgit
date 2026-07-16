#!/usr/bin/env bash
set -euo pipefail

cd "$(dirname "${BASH_SOURCE[0]}")/.."

dependency_tree="$(cargo tree --edges normal --prefix none --format '{p}')"
if grep -Eq '^(rsa|sqlx-mysql|sqlx-macros) v' <<<"$dependency_tree"; then
  echo "error: an excluded dependency is reachable in the build graph" >&2
  exit 1
fi

# Cargo's feature-independent lockfile includes SQLx's disabled
# macros -> MySQL -> RSA optional chain. The graph check above ensures that the
# vulnerable RSA implementation is not reachable by any workspace target.
if [[ -n "${CARGO_AUDIT_BIN:-}" ]]; then
  audit=("$CARGO_AUDIT_BIN" audit)
elif command -v cargo-audit >/dev/null 2>&1; then
  audit=(cargo-audit audit)
else
  audit=(cargo audit)
fi
"${audit[@]}" --deny warnings --ignore RUSTSEC-2023-0071
