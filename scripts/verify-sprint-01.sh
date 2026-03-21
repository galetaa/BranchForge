#!/usr/bin/env bash
set -euo pipefail

repo_root="$(cd "$(dirname "$0")/.." && pwd)"

required_paths=(
  "$repo_root/crates/plugin_api/src/lib.rs"
  "$repo_root/crates/plugin_host/src/lib.rs"
  "$repo_root/crates/plugin_host/tests/runtime_handshake.rs"
  "$repo_root/crates/plugin_host/tests/runtime_contract.rs"
  "$repo_root/crates/action_engine/src/lib.rs"
  "$repo_root/crates/app_host/src/lib.rs"
  "$repo_root/crates/app_host/tests/runtime_invoke_response_e2e.rs"
  "$repo_root/mvp_dev_pack/06_sprints/sprint-01-plugin-runtime/actions/T05_request_routing_and_invoke.md"
  "$repo_root/mvp_dev_pack/06_sprints/sprint-01-plugin-runtime/tests/T06_contract_tests_runtime.md"
)

for path in "${required_paths[@]}"; do
  if [[ ! -e "$path" ]]; then
    echo "missing required Sprint 01 artifact: $path" >&2
    exit 1
  fi
done

cd "$repo_root"
./scripts/dev-check.sh
cargo test -p plugin_host
cargo test -p app_host

echo "Sprint 01 local verification passed"

