# Repository Guidelines

## Project Structure & Module Organization

Branchforge is a Rust workspace. Core host crates live in `crates/`: `app_host` wires the application, `plugin_host` manages plugin processes, `action_engine` routes actions, `state_store` owns UI state, `job_system` runs background work, `git_service` is the only crate allowed to call the Git CLI, and `ui_shell` renders host-side views. Bundled plugins live in `plugins/*` and should depend on `plugin_api` only. A sample out-of-tree plugin is under `external_plugins/sample_plugin`. Architecture and process notes are in `docs/`.

## Build, Test, and Development Commands

- `cargo check --workspace`: fast validation across all crates.
- `cargo test --workspace`: run the full workspace test suite used by CI.
- `cargo fmt --all --check`: enforce formatting.
- `cargo clippy --workspace --all-targets --all-features -- -D warnings`: fail on lint regressions.
- `./scripts/dev-check.sh`: run the standard local quality gate.
- `cargo run -p app_host` or `./scripts/dev-run-host.sh`: start the host app.
- `./scripts/dev-run-local.sh`: build the host plus bundled plugins for local integration.
- `./scripts/check-deps.sh`: enforce crate boundary rules for plugins.

## Coding Style & Naming Conventions

Use Rust 2024 edition defaults and `rustfmt`. Follow standard Rust naming: `snake_case` for functions/modules, `CamelCase` for types, `SCREAMING_SNAKE_CASE` for constants. Keep `unsafe` out of the codebase; workspace lints forbid it. Do not use `dbg!`, `todo!`, or `unwrap()` in committed code. Keep Git command execution inside `crates/git_service`; plugin crates must not reach into host-side internals.

## Testing Guidelines

Prefer unit tests near the crate they exercise and integration tests in `crates/<crate>/tests/`. Existing suites use descriptive smoke/regression names such as `sprint23_beta_hardening_smoke.rs` and `runtime_handshake.rs`. When changing cross-crate behavior, add or update an integration test and run at least the affected package, for example `cargo test -p plugin_host`.

## Commit & Pull Request Guidelines

Recent history follows Conventional Commits with sprint scope, for example `feat(sprint23): beta hardening for perf and accessibility`. Use `feat`, `fix`, `docs`, or `chore`, and keep the scope precise. Branch names should follow `sXX/<area>-<topic>`. PRs should include scope, risks, verification commands, and docs sync notes. If architecture, contracts, RPC, or user flows change, update the relevant files under `docs/` or the sprint packs in the same PR.
