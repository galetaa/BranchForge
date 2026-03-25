#!/usr/bin/env bash
set -euo pipefail

repo_root="$(cd "$(dirname "$0")/.." && pwd)"
cd "$repo_root"

./scripts/check-deps.sh
cargo test -p plugin_sdk -- --nocapture
cargo test -p plugin_host -- --nocapture
cargo test -p app_host --test sprint22_plugin_extensibility_smoke -- --nocapture

echo "Sprint 22 local verification passed"

