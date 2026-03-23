# Branch Compare + Preflight Foundations

This document captures the Sprint 13 foundations for branch compare, conflict state, and preflight.

## Branch compare model

- Compare is expressed as `base_ref -> head_ref`.
- The host exposes a diff source built from `git diff base..head`.
- Compare state is stored in `StateStore.compare` so UI can surface it.

## Conflict state detection

The host probes the repo for conflict state on repo open and before risky operations:

- merge: `MERGE_HEAD`
- rebase: `rebase-apply` or `rebase-merge`
- cherry-pick: `CHERRY_PICK_HEAD`

When a conflict is active, checkout/rename/delete/commit/tag operations are blocked.

## Preflight / preview contract (alpha)

Public API types in `plugin_api`:

- `ActionPreflightRequest { action_id, context }`
- `ActionPreflightResult { action_id, ok, warnings }`
- `ActionPreview { action_id, title, summary, warnings }`

These are placeholder contracts for future reset/rebase previews.
