# v1.0 Gap Analysis

## Closed Repo-Owned Gaps

- [x] Conflict handling UX: `conflict.list`, `conflict.focus`, resolve/mark/continue/abort flows
- [x] Advanced diff interactions: `index.stage_lines`, `index.unstage_lines`, `file.discard_lines`
- [x] Editable interactive rebase plan: `rebase.plan.create`, `rebase.plan.set_action`, `rebase.plan.move`, `rebase.plan.clear`, `rebase.execute`
- [x] Local plugin registry/discovery: `plugin.discover`, `plugin.install_registry`, `plugin_registry/registry.json`
- [x] Packaging/update/rollback/signing automation: `release.package_local`, `release.package`, `release.notes`, `release.sign`
- [x] Documentation backlog: external plugin guide, troubleshooting/recovery guide, compatibility matrix, SDK stabilization guidance

## Runtime Prerequisites

- `git-lfs` binary must be installed to use `diagnostics.lfs_status`, `diagnostics.lfs_fetch`, and `diagnostics.lfs_pull`.
- Production signing should provide `BRANCHFORGE_SIGNING_KEY`; local packaging falls back to an ephemeral dev signature so verification still works in CI/local smoke flows.

## Status

There are no remaining repo-owned v1 backlog items in this document. Historical deferred items have either been implemented or replaced by documented runtime prerequisites.
