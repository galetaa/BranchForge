#!/usr/bin/env bash
set -euo pipefail

repo_root="$(cd "$(dirname "$0")/.." && pwd)"

required_paths=(
  "$repo_root/crates/git_service/src/lib.rs"
  "$repo_root/crates/job_system/src/lib.rs"
  "$repo_root/crates/job_system/tests/git_ops_integration.rs"
  "$repo_root/crates/app_host/src/lib.rs"
  "$repo_root/mvp_dev_pack/06_sprints/sprint-03-git-service-jobs/README.md"
  "$repo_root/mvp_dev_pack/06_sprints/sprint-03-git-service-jobs/git/T01_git_command_runner.md"
  "$repo_root/mvp_dev_pack/06_sprints/sprint-03-git-service-jobs/git/T02_status_porcelain_parser.md"
  "$repo_root/mvp_dev_pack/06_sprints/sprint-03-git-service-jobs/git/T03_repo_open_and_status_handlers.md"
  "$repo_root/mvp_dev_pack/06_sprints/sprint-03-git-service-jobs/jobs/T04_job_queue_and_locks.md"
  "$repo_root/mvp_dev_pack/06_sprints/sprint-03-git-service-jobs/jobs/T05_job_results_and_state_refresh.md"
  "$repo_root/mvp_dev_pack/06_sprints/sprint-03-git-service-jobs/tests/T06_integration_tests_git_ops.md"
)

for path in "${required_paths[@]}"; do
  if [[ ! -e "$path" ]]; then
    echo "missing required Sprint 03 artifact: $path" >&2
    exit 1
  fi
done

cd "$repo_root"
./scripts/dev-check.sh
cargo test -p git_service
cargo test -p job_system
cargo test -p app_host

echo "Sprint 03 local verification passed"

