# Troubleshooting and Recovery Guide

## First-pass checks

1. Run `cargo run -p app_host -- --command "run ops.check_deps"`.
2. Run the relevant runtime verify command or `cargo run -p app_host -- --command "run ops.dev_check"`.
3. Open the diagnostics panel and inspect plugin inventory, journal summary, repo capabilities, and LFS status.

## Plugin issues

- Discovery failures: run `plugin discover plugin_registry` and confirm every `package_dir` resolves to a package containing `plugin.json`.
- Compatibility failures: compare host protocol and plugin `protocol_version`.
- Stale selection: `plugin list` refreshes installed inventory and clears removed plugins from host selection state.

## Conflict and rebase recovery

- Use `conflict.list` to collect current conflicted files.
- Use `conflict.focus <path>` to load a single-file diff before choosing `conflict.resolve.ours` or `conflict.resolve.theirs`.
- Use `rebase.plan.create`, `rebase.plan.set_action`, and `rebase.plan.move` before `rebase.execute` when a rebase needs manual shaping.
- Use `merge.abort`, `rebase.abort`, or `conflict.abort` if the active session must be unwound.

## Packaging and release recovery

- Rebuild a signed local package with `cargo run -p app_host -- --command "run release.package_local"`.
- Produce a release archive with `cargo run -p app_host -- --command "run release.package"`.
- Verify detached checksums:

```bash
openssl dgst -sha256 -verify ./sha256sums.pub -signature ./sha256sums.sig ./sha256sums.txt
```

- Use `rollback.json` in the package root to identify the expected rollback baseline.

## LFS

`diagnostics.lfs_status`, `diagnostics.lfs_fetch`, and `diagnostics.lfs_pull` require the external `git-lfs` binary. If `git lfs version` fails, install `git-lfs` first and rerun the diagnostics op.
