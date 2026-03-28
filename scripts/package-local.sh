#!/usr/bin/env bash
set -euo pipefail

repo_root="$(cd "$(dirname "$0")/.." && pwd)"
cd "$repo_root"

out_dir="$repo_root/target/tmp/local-package"

if [[ "${1:-}" != "" ]]; then
  out_dir="$1"
fi

channel="${BRANCHFORGE_RELEASE_CHANNEL:-local}"
rollback_from="${BRANCHFORGE_ROLLBACK_FROM:-last-stable}"
cargo run -p app_host -- --command "run release.package_local \"$out_dir\" \"$channel\" \"$rollback_from\""
