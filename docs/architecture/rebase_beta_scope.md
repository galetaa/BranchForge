# Interactive Rebase Support

This document records the current interactive rebase behavior after the v1 stabilization pass.

## Supported flow

1. Request `rebase.interactive` for preflight + preview.
2. Create an editable plan with `rebase.plan.create`.
3. Adjust entries with `rebase.plan.set_action`, `rebase.plan.move`, and `rebase.plan.clear`.
4. Execute with `rebase.execute`.
5. Recover active sessions with `rebase.continue`, `rebase.skip`, `rebase.abort`, and `conflict.focus`.

## Current UX model

- host-side console flow with typed rebase plan/session state
- plan execution with autosquash awareness
- restart/session recovery from Git rebase state
- conflict routing through `conflict.list`, `conflict.focus`, resolve/mark/continue/abort actions

## Notes

- `rebase.interactive` is no longer beta-gated.
- The current product surface is console-first rather than a separate full-screen visual editor.
