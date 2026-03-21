#!/usr/bin/env bash
set -euo pipefail

repo_root="$(cd "$(dirname "$0")/.." && pwd)"

required_paths=(
  "$repo_root/plugins/status/src/main.rs"
  "$repo_root/crates/git_service/src/lib.rs"
  "$repo_root/crates/job_system/src/lib.rs"
  "$repo_root/crates/app_host/src/lib.rs"
  "$repo_root/crates/app_host/tests/open_repo_flow_smoke.rs"
  "$repo_root/crates/app_host/tests/mvp_smoke_suite.rs"
  "$repo_root/mvp_dev_pack/06_sprints/sprint-06-commit-and-rc/README.md"
  "$repo_root/mvp_dev_pack/06_sprints/sprint-06-commit-and-rc/actions/T01_commit_action.md"
  "$repo_root/mvp_dev_pack/06_sprints/sprint-06-commit-and-rc/ui/T02_commit_message_prompt_and_feedback.md"
  "$repo_root/mvp_dev_pack/06_sprints/sprint-06-commit-and-rc/qa/T03_smoke_suite_for_mvp.md"
)

for path in "${required_paths[@]}"; do
  if [[ ! -e "$path" ]]; then
    echo "missing required Sprint 06 artifact: $path" >&2
    exit 1
  fi
done

cd "$repo_root"
./scripts/dev-check.sh
cargo test -p git_service
cargo test -p job_system
cargo test -p app_host
cargo test -p app_host --test mvp_smoke_suite
cargo test -p plugin_host
cargo test -p status

echo "Sprint 06 local verification passed"

