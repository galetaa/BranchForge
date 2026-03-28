# Branchforge

Current state: feature-complete Rust workspace through sprint 24, with an interactive console runner in `app_host`.

## Quick start

```bash
cargo run -p app_host -- --command "run ops.check_deps"
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

Use `ops` inside the runner to print the full direct op catalog across history, diff, staging, stash, worktree, submodule, branch/rebase/conflict recovery, diagnostics, plugin lifecycle, and runtime packaging/verification flows.

One-shot runtime mode is also available for operational tasks:

```bash
cargo run -p app_host -- --command "run ops.dev_check"
cargo run -p app_host -- --command "run release.package_local"
cargo run -p app_host -- --command "run verify.sprint24"
```

## GUI runtime

Branchforge now also ships a browser-based GUI host on top of the same runtime/state stack:

Run it like this:

```bash
cargo run -p app_gui
# optional custom bind
cargo run -p app_gui -- --bind 127.0.0.1:8787
```

Startup flow:

1. Start the server with `cargo run -p app_gui`.
2. Wait for `Branchforge GUI listening on http://127.0.0.1:8787`.
3. Open `http://127.0.0.1:8787` in your browser.
4. Stop the GUI server with `Ctrl+C` in the terminal when finished.

The GUI reuses the existing host runtime for `open`, `panel`, `select`, `run`, `refresh`, plugin lifecycle, diagnostics, and the rest of the direct op catalog. It is not a mock or separate backend.
The current revision exposes the full `app_host` direct-op surface through dedicated GUI widgets, panel actions, selection shortcuts, or the command box, including history search, stash/worktree/submodule flows, branch/tag management, merge/cherry-pick/revert/reset, rebase/conflict recovery, plugin registry/install lifecycle, LFS diagnostics, release/sign/verify runtime operations, and diff hunk or line actions.

## Local dev helpers

```bash
cargo run -p app_host -- --command "run ops.dev_check"
./scripts/dev-run-host.sh
./scripts/dev-run-local.sh
```

Local package layout (host + bundled plugins):

```bash
cargo run -p app_host -- --command "run release.package_local"
```

Artifacts land in `target/tmp/local-package`.

Compatibility shell wrappers remain in `scripts/`, but they now delegate to runtime host operations instead of carrying packaging or verification logic directly.

## Workspace layout

- `crates/` host-side crates
- `crates/app_gui` browser-based GUI host
- `plugins/` bundled plugin executables
- `docs/` architecture and delivery rules
- `docs/process/console_runner_usage.md` interactive runner command guide
- `docs/process/gui_runtime_usage.md` GUI runtime guide
