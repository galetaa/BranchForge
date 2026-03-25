#!/usr/bin/env bash
set -euo pipefail

repo_root="$(cd "$(dirname "$0")/.." && pwd)"
cd "$repo_root"

./scripts/check-deps.sh
cargo test -p git_service -- --nocapture
cargo test -p job_system -- --nocapture
cargo test -p ui_shell -- --nocapture
cargo test -p app_host --test sprint18_conflict_recovery_smoke -- --nocapture

echo "Sprint 18 local verification passed"

