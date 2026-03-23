# Beta Release Plan (Advanced Workflows)

## Channels

- `beta`: opt-in for advanced workflows (rebase/compare/conflicts)
- `stable`: default channel with conservative features

## Known issues process

- Maintain a rolling list of beta-known issues
- Require a mitigation or workaround for each item
- Review weekly during beta window

## Rollback policy

- Beta builds can be rolled back to last stable build
- Beta features should have explicit feature flags
- Any data migration must be reversible

## Checklist (draft)

- Feature flags wired for beta-only features
- Release notes template updated
- Support escalation path defined
