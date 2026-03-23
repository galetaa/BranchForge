#!/usr/bin/env bash
set -euo pipefail

repo_root="$(cd "$(dirname "$0")/.." && pwd)"
cd "$repo_root"

./scripts/check-deps.sh
cargo fmt --all --check
cargo clippy --workspace --all-targets --all-features -- -D warnings
cargo test -p git_service
cargo test -p plugin_host
cargo test -p job_system
cargo test -p app_host --test history_diff_e2e

echo "Sprint 08 local verification passed"
