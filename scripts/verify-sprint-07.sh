#!/usr/bin/env bash
set -euo pipefail

repo_root="$(cd "$(dirname "$0")/.." && pwd)"
cd "$repo_root"

./scripts/check-deps.sh
cargo fmt --all --check
cargo test -p git_service
cargo test -p plugin_host
cargo test -p job_system
cargo test -p app_host --test post_mvp_smoke_baseline

echo "Sprint 07 local verification passed"
