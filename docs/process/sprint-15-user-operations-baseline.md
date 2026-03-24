# Sprint 15 User Operations Baseline

This note documents user-facing behavior introduced in Sprint 15 for destructive and ref-management operations.

## Discard file changes (`file.discard`)

- Scope: full-file discard of worktree changes.
- Safety baseline: destructive operation; requires explicit confirmation in host policy.
- Expected result: selected file content is restored from `HEAD` (or index baseline), status refresh runs immediately.

Recommended UI copy:

- Title: `Discard file changes?`
- Body: `This restores selected files to repository state and cannot be undone from Branchforge.`
- Confirm: `Discard changes`
- Cancel: `Keep changes`

## Delete tag (`tag.delete`)

- Scope: local tag delete.
- Safety baseline: mutating refs operation with medium danger profile.
- Expected result: tag is removed, refs/tags list refresh runs immediately.

Recommended UI copy:

- Title: `Delete tag?`
- Body: `The selected local tag will be removed from this repository.`
- Confirm: `Delete tag`
- Cancel: `Keep tag`

## Error handling baseline

- User-facing errors should include normalized category and correlation id.
- Correlation id format: `bf-<timestamp_ms>-<seq>`.
- Support/debug flow should request correlation id first, then inspect logs/journal.

