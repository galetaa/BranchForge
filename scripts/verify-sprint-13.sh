#!/usr/bin/env bash
set -euo pipefail

repo_root="$(cd "$(dirname "$0")/.." && pwd)"
cd "$repo_root"

./scripts/check-deps.sh
cargo fmt --all --check
cargo clippy --workspace --all-targets --all-features -- -D warnings
cargo test -p git_service
cargo test -p plugin_host
cargo test -p action_engine
cargo test -p job_system
cargo test -p app_host --test commit_polish_search_tags_smoke
cargo test -p app_host --test safety_regression_suite
cargo test -p app_host --test foundation_regression_suite

echo "Sprint 13 local verification passed"
