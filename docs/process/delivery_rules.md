# Delivery rules

## Definition of Done

1. Code is implemented and builds.
2. Tests exist on required level.
3. No new fmt/clippy violations.
4. Documentation is synchronized.
5. Acceptance criteria are met.

## Task format

Each task must include:

- context
- objective
- dependencies
- acceptance criteria
- verifiable artifact list
- out of scope
- verification commands

## Naming conventions

- Branch: `sXX/<area>-<short-topic>`
- PR title: `[SXX] <area>: <change>`
- PR body must include: scope, risks, verification, docs sync
- Conventional commits:
  - `feat(scope): ...`
  - `fix(scope): ...`
  - `docs(scope): ...`
  - `chore(scope): ...`

## Documentation update rule

If a task changes architecture, contracts, rpc, or flows, the relevant docs under `mvp_dev_pack/` or `docs/` must be updated in the same PR.

## Templates

- Issue template: `.github/ISSUE_TEMPLATE/task.md`


