#!/usr/bin/env bash
set -euo pipefail

repo_root="$(cd "$(dirname "$0")/.." && pwd)"
cd "$repo_root"

out_dir="${1:-$repo_root/target/tmp/beta-package}"
channel="${BRANCHFORGE_RELEASE_CHANNEL:-beta}"
rollback_from="${BRANCHFORGE_ROLLBACK_FROM:-last-stable}"

cargo run -p app_host -- --command "run release.verify \"$out_dir\" \"$channel\" \"$rollback_from\""
