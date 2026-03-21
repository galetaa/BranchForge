# Sprint 02 Report (UI Shell + State)

Date: 2026-03-21
Status: PASS (local), PENDING remote CI proof

## Scope

Sprint 02 goal: deliver host UI shell, command palette, typed state store, and host-rendered ViewModel for status panel.

Reference: `mvp_dev_pack/06_sprints/sprint-02-ui-shell-state/README.md`

## Exit Criteria Check

- [x] Host renders window layout with stable left slot.
  - Evidence: `crates/ui_shell/src/layout.rs`, `crates/app_host/tests/ui_state_contract_smoke.rs`
- [x] Command palette is available and can invoke action.
  - Evidence: `crates/ui_shell/src/palette.rs`, `crates/app_host/src/lib.rs`, `crates/app_host/tests/ui_state_contract_smoke.rs`
- [x] Typed snapshots are stored with versioning and subscriptions.
  - Evidence: `crates/state_store/src/lib.rs` and its unit tests
- [x] Status panel is rendered from host-side viewmodel.
  - Evidence: `crates/ui_shell/src/viewmodel.rs`, `crates/ui_shell/src/lib.rs`

## Task-Level Coverage (Sprint 02)

- T01 Window layout and slots: completed.
  - Evidence: `crates/ui_shell/src/layout.rs`, `crates/ui_shell/src/lib.rs`, `crates/app_host/src/lib.rs`
- T02 Command palette: completed.
  - Evidence: `crates/ui_shell/src/palette.rs`, `crates/app_host/src/lib.rs`, `crates/app_host/tests/ui_state_contract_smoke.rs`
- T03 Typed state store: completed.
  - Evidence: `crates/state_store/src/lib.rs`
- T04 Event bus and state notifications: completed (runtime subscription smoke path).
  - Evidence: `crates/state_store/src/lib.rs`, `crates/plugin_host/src/lib.rs`, `crates/app_host/src/lib.rs`
- T05 ViewModel renderer v0.1: completed.
  - Evidence: `crates/ui_shell/src/viewmodel.rs`, `crates/ui_shell/src/lib.rs`
- T06 UI-state contract smoke: completed.
  - Evidence: `crates/app_host/tests/ui_state_contract_smoke.rs`

## Quality Gates

Latest local run result: PASS.

- `./scripts/verify-sprint-02.sh`
- `./scripts/dev-check.sh`
- `cargo test -p state_store`
- `cargo test -p ui_shell`
- `cargo test -p app_host`

CI gate definition:

- PR and push-to-main triggers configured in `.github/workflows/ci.yml`

## Risks and Follow-ups

- Current UI shell is text-rendered smoke model; real window framework integration remains a later increment.
- `when` evaluation supports only `always` and `repo.is_open` in Sprint 02 scope.
- Event delivery path is currently host/runtime smoke wiring and should be expanded with richer runtime subscriptions later.

## Final Closure Checklist

- [x] Local verification is green via `scripts/verify-sprint-02.sh`.
- [ ] Remote CI run URL is attached.
- [ ] Commit SHA for closure is attached.

## Remote CI Proof (fill after push)

- CI run URL: `<paste-url-here>`
- Commit SHA: `<paste-sha-here>`
- CI result: `PASS`
- Verified at (UTC): `<YYYY-MM-DD HH:MM>`

## Closure Decision

Sprint 02 UI Shell + State is locally complete and ready for handoff to Sprint 03.
Final procedural closure requires attaching one green remote CI run URL and commit SHA.

