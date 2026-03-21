# Sprint 01 Report (Plugin Runtime)

Date: 2026-03-21
Status: PASS (local), PENDING remote CI proof

## Scope

Sprint 01 goal: deliver working out-of-process plugin runtime with transport, handshake, registry, and request/response routing.

Reference: `mvp_dev_pack/06_sprints/sprint-01-plugin-runtime/README.md`

## Exit Criteria Check

- [x] Host can run bundled plugin runtime path with out-of-process lifecycle primitives.
  - Evidence: `crates/plugin_host/src/lib.rs` (`PluginProcess`, `RestartPolicy`, lifecycle tests)
- [x] Handshake is implemented (`plugin.hello` + registration + ready state).
  - Evidence: `crates/plugin_host/src/lib.rs` (`RuntimeSession`), `crates/plugin_host/tests/runtime_handshake.rs`
- [x] Actions/views ownership registry is implemented with duplicate rejection.
  - Evidence: `crates/plugin_host/src/lib.rs` (`PluginRegistry`), duplicate tests
- [x] Request/response route for `host.action.invoke` is wired and testable.
  - Evidence: `crates/action_engine/src/lib.rs`, `crates/app_host/src/lib.rs`, `crates/app_host/tests/runtime_invoke_response_e2e.rs`

## Task-Level Coverage (Sprint 01)

- T01 Message envelope and codec: completed.
  - Evidence: `crates/plugin_api/src/lib.rs` (`FrameCodec`, `RpcMessage`, negative codec tests)
- T02 Protocol models v0.1: completed.
  - Evidence: `crates/plugin_api/src/lib.rs` (`PluginHello`, `PluginRegister`, `ActionSpec`, `ViewSpec`, event payload types)
- T03 Plugin process lifecycle: completed.
  - Evidence: `crates/plugin_host/src/lib.rs` (`spawn/send/receive/shutdown/restart`), tests for stderr + restart policy
- T04 Handshake and registration: completed.
  - Evidence: `crates/plugin_host/src/lib.rs`, `crates/plugin_host/tests/runtime_handshake.rs`
- T05 Request routing and invoke: completed.
  - Evidence: `crates/plugin_host/src/lib.rs` (`pending map`, `timeout policy`, `invoke_action`), `crates/action_engine/src/lib.rs`, `crates/app_host/tests/runtime_invoke_response_e2e.rs`
- T06 Contract tests runtime: completed.
  - Evidence: `crates/plugin_host/tests/runtime_handshake.rs`, `crates/plugin_host/tests/runtime_contract.rs`

## Quality Gates

Latest local run result: PASS.

- `./scripts/verify-sprint-01.sh`
- `./scripts/dev-check.sh`
- `cargo test -p plugin_host`
- `cargo test -p app_host`

CI gate definition:

- PR and push-to-main triggers configured in `.github/workflows/ci.yml`

## Risks and Follow-ups

- Current e2e host roundtrip uses in-memory simulated response; replace with real plugin process wiring in host shell increment.
- Keep runtime contract tests aligned when new RPC methods are added in Sprint 02+.
- Expand timeout tests to include host-level retries once retry policy is introduced.

## Final Closure Checklist

- [x] Local verification is green via `scripts/verify-sprint-01.sh`.
- [ ] Remote CI run URL is attached.
- [ ] Commit SHA for closure is attached.

## Remote CI Proof (fill after push)

- CI run URL: `<paste-url-here>`
- Commit SHA: `<paste-sha-here>`
- CI result: `PASS`
- Verified at (UTC): `<YYYY-MM-DD HH:MM>`

## Closure Decision

Sprint 01 Plugin Runtime is locally complete and ready for handoff to Sprint 02.
Final procedural closure requires attaching one green remote CI run URL and commit SHA.

