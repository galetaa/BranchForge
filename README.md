# Branchforge

Current state: Sprint 06 (Commit + Release Candidate) implemented locally on top of Sprint 00/01/02/03/04/05 foundations.

Reports:

- `SPRINT_00_REPORT.md`
- `SPRINT_01_REPORT.md`
- `SPRINT_02_REPORT.md`
- `SPRINT_03_REPORT.md`
- `SPRINT_04_REPORT.md`
- `SPRINT_05_REPORT.md`
- `SPRINT_06_REPORT.md`

## Quick start

```bash
./scripts/verify-sprint-06.sh
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
