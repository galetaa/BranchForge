#!/usr/bin/env bash
set -euo pipefail

repo_root="$(cd "$(dirname "$0")/.." && pwd)"

required_paths=(
  "$repo_root/crates/state_store/src/lib.rs"
  "$repo_root/crates/ui_shell/src/lib.rs"
  "$repo_root/crates/ui_shell/src/viewmodel.rs"
  "$repo_root/crates/ui_shell/src/palette.rs"
  "$repo_root/crates/ui_shell/src/layout.rs"
  "$repo_root/crates/app_host/src/lib.rs"
  "$repo_root/crates/app_host/tests/ui_state_contract_smoke.rs"
  "$repo_root/mvp_dev_pack/06_sprints/sprint-02-ui-shell-state/ui-shell/T01_window_layout_and_slots.md"
  "$repo_root/mvp_dev_pack/06_sprints/sprint-02-ui-shell-state/ui-shell/T02_command_palette.md"
  "$repo_root/mvp_dev_pack/06_sprints/sprint-02-ui-shell-state/state/T03_state_models_and_store.md"
  "$repo_root/mvp_dev_pack/06_sprints/sprint-02-ui-shell-state/state/T04_event_bus_and_state_notifications.md"
  "$repo_root/mvp_dev_pack/06_sprints/sprint-02-ui-shell-state/viewmodel/T05_viewmodel_renderer_v0_1.md"
  "$repo_root/mvp_dev_pack/06_sprints/sprint-02-ui-shell-state/tests/T06_ui_state_contract_smoke.md"
)

for path in "${required_paths[@]}"; do
  if [[ ! -e "$path" ]]; then
    echo "missing required Sprint 02 artifact: $path" >&2
    exit 1
  fi
done

cd "$repo_root"
./scripts/dev-check.sh
cargo test -p state_store
cargo test -p ui_shell
cargo test -p app_host

echo "Sprint 02 local verification passed"

