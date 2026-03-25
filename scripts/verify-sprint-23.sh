#!/usr/bin/env bash
set -euo pipefail

repo_root="$(cd "$(dirname "$0")/.." && pwd)"
cd "$repo_root"

./scripts/check-deps.sh
cargo test -p state_store -- --nocapture
cargo test -p ui_shell -- --nocapture
cargo test -p app_host --test sprint23_beta_hardening_smoke -- --nocapture

package_out="$repo_root/target/tmp/sprint23-package-check"
./scripts/package-local.sh "$package_out"
[[ -f "$package_out/manifest.txt" ]]
[[ -f "$package_out/bin/app_host" ]]

echo "Sprint 23 local verification passed"

