#!/usr/bin/env bash
set -euo pipefail

repo_root="$(cd "$(dirname "$0")/.." && pwd)"

cd "$repo_root"
cargo build -p app_host -p repo_manager -p status

exec "$repo_root/target/debug/app_host"
