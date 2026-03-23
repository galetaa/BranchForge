# Interactive Rebase Beta Scope

This document captures the limited beta scope for interactive rebase in Sprint 14.

## In scope

- Beta flag gate (`BRANCHFORGE_REBASE_BETA=1`)
- Preflight + preview contract integration
- Basic plan surface (no execution yet)

## Out of scope

- Full interactive rebase UI
- Conflict resolution UI
- General availability

## User-facing flow (beta)

1. Enable beta flag.
2. Request `rebase.interactive` action.
3. Host runs preflight and returns preview summary.

## Notes

This beta is intended for controlled testing only.
