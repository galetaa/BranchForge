# Branchforge

Current state: feature-complete Rust workspace through sprint 24, with an interactive console runner in `app_host`.

## Quick start

```bash
./scripts/check-deps.sh
cargo check --workspace
cargo test --workspace
```

## Interactive console runner

Start the host REPL/TUI-style runner:

```bash
cargo run -p app_host
# or
./scripts/dev-run-host.sh
```

The runner is a console layer on top of the existing `action_engine`, `job_system`, `state_store`, and `ui_shell` stack. Example session:

```text
open .
panel history
actions
run diagnostics.repo_capabilities
select file Cargo.toml
run index.stage_selected
run diff.index
show
quit
```

Use `run --confirm ...` or `run ... --confirm` for destructive actions such as `branch.delete`, `reset.hard`, or `rebase.interactive`.

`panel diagnostics` shows the existing sprint 23 diagnostics plus host-side plugin inventory from sprint 22. The diagnostics palette now also includes host-side `plugin.*` actions, `select plugin <id>` is stored in shared host state, and both `run plugin.enable|disable|remove` and `plugin disable|remove` can reuse the current plugin selection. Destructive plugin commands accept both `plugin --confirm remove` and `plugin remove --confirm`, and stale plugin selection is cleared automatically on the next inventory sync.

Use `ops` inside the runner to print the full direct op catalog across history, diff, staging, stash, worktree, submodule, branch/rebase/conflict recovery, diagnostics, and plugin lifecycle flows.

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
- `docs/process/console_runner_usage.md` interactive runner command guide
