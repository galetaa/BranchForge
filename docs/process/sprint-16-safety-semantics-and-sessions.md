# Sprint 16 Safety Semantics and Operation Sessions

This document defines baseline user-facing safety semantics for advanced branch operations introduced in Sprint 16.

## Merge semantics

Supported merge modes:

- `ff` (fast-forward only)
- `no-ff`
- `squash`

Safety baseline:

- Show source and target refs before execution.
- Show merge mode explicitly.
- When conflict state appears, expose conflict route copy:
  - merge: `resolve conflicts or run merge.abort`
  - cherry-pick: `resolve conflicts or run cherry_pick.abort`

## Reset semantics

Supported reset modes:

- `soft`: moves `HEAD`, preserves index and worktree.
- `mixed`: moves `HEAD`, resets index, preserves worktree.
- `hard`: moves `HEAD`, resets index, drops worktree changes.

Safety baseline:

- Always render impact explanation for selected mode.
- `hard` reset is destructive and requires explicit confirmation.

## Operation sessions

Advanced operations run with operation-session metadata captured in operation journal entries:

- `session_id`
- `session_kind`
- `session_state` (`running`, `succeeded`, `failed`)
- `pre_refs` snapshot summary
- `post_refs` snapshot summary

UI baseline:

- Show in-progress operation banner when repository conflict state is present.
- Show session badge with latest session id/op/state.

## Diagnostics and support

For advanced operation incidents:

1. Request correlation id (from normalized errors).
2. Inspect operation journal entry by `session_id`.
3. Compare `pre_refs` and `post_refs` snapshots.

