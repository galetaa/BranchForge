#!/usr/bin/env bash
set -euo pipefail

repo_root="$(cd "$(dirname "$0")/.." && pwd)"
cd "$repo_root"

./scripts/check-deps.sh
cargo test -p git_service -- --nocapture
cargo test -p ui_shell -- --nocapture
cargo test -p app_host --test sprint17_interactive_rebase_smoke -- --nocapture

echo "Sprint 17 local verification passed"

