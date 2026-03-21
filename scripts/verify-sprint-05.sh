#!/usr/bin/env bash
set -euo pipefail

repo_root="$(cd "$(dirname "$0")/.." && pwd)"

required_paths=(
  "$repo_root/plugins/status/src/main.rs"
  "$repo_root/crates/git_service/src/lib.rs"
  "$repo_root/crates/job_system/src/lib.rs"
  "$repo_root/crates/app_host/src/lib.rs"
  "$repo_root/crates/app_host/tests/open_repo_flow_smoke.rs"
  "$repo_root/mvp_dev_pack/06_sprints/sprint-05-status-plugin/README.md"
  "$repo_root/mvp_dev_pack/06_sprints/sprint-05-status-plugin/plugin/T01_status_plugin_registration_and_view.md"
  "$repo_root/mvp_dev_pack/06_sprints/sprint-05-status-plugin/plugin/T02_selection_flow_from_lists.md"
  "$repo_root/mvp_dev_pack/06_sprints/sprint-05-status-plugin/actions/T03_stage_selected_action.md"
  "$repo_root/mvp_dev_pack/06_sprints/sprint-05-status-plugin/actions/T04_unstage_selected_action.md"
  "$repo_root/mvp_dev_pack/06_sprints/sprint-05-status-plugin/refresh/T05_status_reactivity_and_refresh.md"
  "$repo_root/mvp_dev_pack/06_sprints/sprint-05-status-plugin/tests/T06_status_stage_unstage_e2e.md"
)

for path in "${required_paths[@]}"; do
  if [[ ! -e "$path" ]]; then
    echo "missing required Sprint 05 artifact: $path" >&2
    exit 1
  fi
done

cd "$repo_root"
./scripts/dev-check.sh
cargo test -p git_service
cargo test -p job_system
cargo test -p app_host
cargo test -p status

echo "Sprint 05 local verification passed"

