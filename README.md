# Branchforge

Sprint 00 bootstraps a Rust workspace with crate boundaries, developer tooling, and CI quality gates.

## Quick start

```bash
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


