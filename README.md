# Branchforge

Current state: Sprint 07 (Stabilization and Hardening) in progress on top of MVP foundations.

## Quick start

```bash
./scripts/verify-sprint-07.sh
./scripts/check-deps.sh
cargo check --workspace
cargo test --workspace
```

## Local dev helpers

```bash
./scripts/dev-check.sh
./scripts/dev-run-host.sh
./scripts/dev-run-local.sh
```

Local package layout (host + bundled plugins):

```bash
./scripts/package-local.sh
```

Artifacts land in `target/tmp/local-package`.

## Workspace layout

- `crates/` host-side crates
- `plugins/` bundled plugin executables
- `docs/` architecture and delivery rules
