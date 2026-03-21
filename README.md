# Branchforge

Current state: Sprint 01 (Plugin Runtime) implemented locally on top of Sprint 00 foundation.

Reports:

- `SPRINT_00_REPORT.md`
- `SPRINT_01_REPORT.md`

## Quick start

```bash
./scripts/verify-sprint-01.sh
./scripts/check-deps.sh
cargo check --workspace
cargo test --workspace
```

## Local dev helpers

```bash
./scripts/dev-check.sh
./scripts/dev-run-host.sh
```

## Workspace layout

- `crates/` host-side crates
- `plugins/` bundled plugin executables
- `docs/` architecture and delivery rules


