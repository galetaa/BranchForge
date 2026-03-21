#!/usr/bin/env bash
set -euo pipefail

repo_root="$(cd "$(dirname "$0")/.." && pwd)"

required_paths=(
  "$repo_root/Cargo.toml"
  "$repo_root/rust-toolchain.toml"
  "$repo_root/.cargo/config.toml"
  "$repo_root/.github/workflows/ci.yml"
  "$repo_root/docs/architecture/crate_boundaries.md"
  "$repo_root/docs/process/delivery_rules.md"
  "$repo_root/.github/ISSUE_TEMPLATE/task.md"
  "$repo_root/scripts/check-deps.sh"
  "$repo_root/scripts/dev-check.sh"
)

for path in "${required_paths[@]}"; do
  if [[ ! -e "$path" ]]; then
    echo "missing required Sprint 00 artifact: $path" >&2
    exit 1
  fi
done

cd "$repo_root"
./scripts/dev-check.sh

echo "Sprint 00 local verification passed"



