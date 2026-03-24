# Sprint 17 Interactive Rebase Concepts and Limits

This note defines baseline behavior for interactive rebase in Sprint 17.

## RebasePlan lifecycle

1. Create plan from selected base (`rebase.plan.create`).
2. Edit ordered entries (reorder, squash, fixup, edit, drop).
3. Execute plan (`rebase.execute`).
4. Resolve session transitions via `rebase.continue`, `rebase.skip`, `rebase.abort`.

## Safety model

- Interactive rebase rewrites history and uses high-risk confirmations.
- If branch has upstream tracking, surface published-history warning before execution.
- Autosquash markers (`fixup!`, `squash!`) are detected and shown in plan metadata.

## Session recovery

- Host detects restart-time rebase hooks from Git state (`rebase-merge` / `rebase-apply`).
- Session snapshot fields:
  - `active`
  - `current_step`
  - `total_steps`
  - `blocking_conflict`

## Current limitations (Sprint 17 baseline)

- Rebase plan uses direct todo script injection; no full visual editor persistence yet.
- Merge-commit parent-selection for advanced rewrite variants is out of scope.
- Autosquash baseline is awareness + execution flag, not full auto-plan transformation.

