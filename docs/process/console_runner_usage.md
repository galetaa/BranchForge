# Console Runner Usage

`app_host` now starts an interactive console runner instead of exiting after a smoke print. The runner is a thin host-side layer over the existing `action_engine`, `job_system`, `state_store`, and `ui_shell` stack; it does not replace the underlying action/job architecture.

## Start

```bash
cargo run -p app_host
```

Optional:

```bash
BRANCHFORGE_PLUGINS_ROOT=./target/tmp/console-plugins cargo run -p app_host
```

One-shot runtime commands are also supported:

```bash
cargo run -p app_host -- --command "run ops.dev_check"
cargo run -p app_host -- --command "run release.package_local"
cargo run -p app_host -- --command "run verify.sprint24"
```

## Core commands

- `help`, `show`, `quit`
- `open <path>`: open a repository and hydrate status/refs state.
- `panel <status|history|branches|tags|compare|diagnostics>`: switch the rendered panel.
- `actions`: list registered action ids with enabled/disabled state.
- `ops`: print the full direct job/host op catalog accepted by `run`, grouped by feature area.
- `run <action_or_op> [args...]`: execute an action id or direct job op.
- `run --confirm <action_or_op> [args...]`: required for destructive actions.
- `run ... --confirm`: postfix confirmation is also accepted.
- `select file <path>`, `select commit <oid>`, `select branch <name>`, `select plugin <id>`: update selection state.
- `refresh`: rerun status/refs refresh and replay the last read-only context op.
- `plugin list|discover|install|install-registry|enable|disable|remove ...`: manage local plugins and registry discovery from the host. Registry sources may be local paths, `file://...`, or `http://...` URLs.
- `plugin --confirm ...` and `plugin ... --confirm`: both confirmation forms are accepted for destructive plugin lifecycle commands.

`panel diagnostics` and `run diagnostics.journal_summary` render the existing diagnostics/perf state and also show the installed plugin inventory tracked by the host-side console layer. The diagnostics action list/palette now includes host-side `plugin.*` actions, and the selected plugin is highlighted from shared host state. If a selected plugin disappears from disk, the next inventory sync (`panel diagnostics` or `plugin list`) clears the stale selection automatically.

Advanced runner flows now include:

- line-level diff actions: `index.stage_lines`, `index.unstage_lines`, `file.discard_lines`
- interactive rebase plan editing: `rebase.plan.set_action`, `rebase.plan.move`, `rebase.plan.clear`
- conflict focus routing: `conflict.focus <path>`
- optional LFS diagnostics/workflow ops: `diagnostics.lfs_status`, `diagnostics.lfs_fetch`, `diagnostics.lfs_pull`
- runtime operational flows: `ops.check_deps`, `ops.dev_check`, `release.notes`, `release.package_local`, `release.package`, `release.sign`, `verify.sprint22`, `verify.sprint23`, `verify.sprint24`

Several `run` commands can now reuse current selection when args are omitted:

- selected commit: `history.select_commit`, `history.details`, `diff.commit`, `cherry_pick.commit`, `revert.commit`
- selected file(s): `history.file`, `blame.file`, `diff.worktree`, `diff.index`, `index.stage_paths`, `index.unstage_paths`, `file.discard`, conflict resolution ops
- selected branch: `branch.checkout`, `branch.delete`, `branch.rename`, `merge.execute`, `compare.refs`
- selected plugin: `plugin enable`, `plugin disable`, `plugin remove`, `plugin.enable`, `plugin.disable`, `plugin.remove`

## Example flows

```text
open .
panel history
run history.page 0 20
select commit <oid>
run diff.commit
```

```text
select file Cargo.toml
run index.stage_selected
run diff.index
run index.unstage_hunk Cargo.toml 0
run index.unstage_lines Cargo.toml 0 0 1
```

```text
panel diagnostics
plugin list
plugin discover plugin_registry
plugin install-registry sample_external plugin_registry
select plugin sample_status
plugin disable
plugin --confirm remove
```

```text
select branch feature/demo
run compare.refs
show
```

```text
run --confirm rebase.interactive main autosquash
run rebase.plan.create main
run rebase.plan.set_action 1 squash
run rebase.plan.move 1 0
run rebase.execute
run conflict.list
run conflict.focus src/lib.rs
run conflict.resolve.ours src/lib.rs
run conflict.mark_resolved
run conflict.continue
```
