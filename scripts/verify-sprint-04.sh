#!/usr/bin/env bash
set -euo pipefail

repo_root="$(cd "$(dirname "$0")/.." && pwd)"

required_paths=(
  "$repo_root/plugins/repo_manager/src/main.rs"
  "$repo_root/crates/plugin_host/src/lib.rs"
  "$repo_root/crates/app_host/src/lib.rs"
  "$repo_root/crates/app_host/src/recent_repos.rs"
  "$repo_root/crates/app_host/tests/open_repo_flow_smoke.rs"
  "$repo_root/mvp_dev_pack/06_sprints/sprint-04-repo-manager-plugin/README.md"
  "$repo_root/mvp_dev_pack/06_sprints/sprint-04-repo-manager-plugin/plugin/T01_repo_manager_plugin_registration.md"
  "$repo_root/mvp_dev_pack/06_sprints/sprint-04-repo-manager-plugin/plugin/T02_open_repo_action_flow.md"
  "$repo_root/mvp_dev_pack/06_sprints/sprint-04-repo-manager-plugin/ui/T03_empty_state_and_open_hint.md"
  "$repo_root/mvp_dev_pack/06_sprints/sprint-04-repo-manager-plugin/storage/T04_recent_repos_storage.md"
  "$repo_root/mvp_dev_pack/06_sprints/sprint-04-repo-manager-plugin/tests/T05_open_repo_e2e_smoke.md"
  "$repo_root/mvp_dev_pack/06_sprints/sprint-04-repo-manager-plugin/qa/T06_error_messages_for_invalid_repo.md"
)

for path in "${required_paths[@]}"; do
  if [[ ! -e "$path" ]]; then
    echo "missing required Sprint 04 artifact: $path" >&2
    exit 1
  fi
done

cd "$repo_root"
./scripts/dev-check.sh
cargo test -p plugin_host
cargo test -p app_host
cargo test -p repo_manager

echo "Sprint 04 local verification passed"

