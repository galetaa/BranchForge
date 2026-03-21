# Sprint 03 Report (Git Service + Jobs)

Date: 2026-03-21
Status: PASS (local), PENDING remote CI proof

## Scope

Sprint 03 goal: integrate git CLI execution, job queue/locks, and state refresh after git operations.

Reference: `mvp_dev_pack/06_sprints/sprint-03-git-service-jobs/README.md`

## Exit Criteria Check

- [x] Host can run job ops v0.1 (`repo.open`, `status.refresh`).
  - Evidence: `crates/job_system/src/lib.rs`, `crates/app_host/src/lib.rs`
- [x] Snapshots are refreshed after git operations.
  - Evidence: `crates/job_system/src/lib.rs`, `crates/state_store/src/lib.rs`
- [x] Lock policy prevents conflicting jobs.
  - Evidence: `crates/job_system/src/lib.rs` (`queue_respects_lock_conflicts`)

## Task-Level Coverage (Sprint 03)

- T01 Safe git command runner: completed.
  - Evidence: `crates/git_service/src/lib.rs` (`run_git`), unit tests
- T02 Porcelain v2 parser: completed.
  - Evidence: `crates/git_service/src/lib.rs` (`parse_status_porcelain_v2_z`), fixture test
- T03 repo.open / status.refresh handlers: completed.
  - Evidence: `crates/git_service/src/lib.rs`, `crates/job_system/src/lib.rs`
- T04 Job queue and locks: completed.
  - Evidence: `crates/job_system/src/lib.rs`
- T05 Job results and state refresh: completed.
  - Evidence: `crates/job_system/src/lib.rs`, `crates/app_host/src/lib.rs`
- T06 Integration tests for git ops: completed (foundation path).
  - Evidence: `crates/job_system/tests/git_ops_integration.rs`, `crates/app_host/src/lib.rs`

## Quality Gates

Latest local run result: PASS.

- `./scripts/verify-sprint-03.sh`
- `./scripts/dev-check.sh`
- `cargo test -p git_service`
- `cargo test -p job_system`
- `cargo test -p app_host`

CI gate definition:

- PR and push-to-main triggers configured in `.github/workflows/ci.yml`

## Risks and Follow-ups

- Current T06 focuses on `repo.open/status.refresh` foundation; stage/commit integration is next step.
- Status parser currently supports main record kinds for MVP and can be extended for additional edge cases.
- Job queue is in-memory and single-process; persistence/retry policy remain out of Sprint 03 scope.

## Final Closure Checklist

- [x] Local verification is green via `scripts/verify-sprint-03.sh`.
- [ ] Remote CI run URL is attached.
- [ ] Commit SHA for closure is attached.

## Remote CI Proof (fill after push)

- CI run URL: `<paste-url-here>`
- Commit SHA: `<paste-sha-here>`
- CI result: `PASS`
- Verified at (UTC): `<YYYY-MM-DD HH:MM>`

## Closure Decision

Sprint 03 Git Service + Jobs is locally complete and ready for handoff to Sprint 04.
Final procedural closure requires attaching one green remote CI run URL and commit SHA.

