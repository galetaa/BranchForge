# Known Issues and Support Guide v1.0.0

## Known limitations
- Line-level staging remains out of scope; hunk-level staging is the baseline
- Blame baseline is text-first and optimized for core workflows, not advanced annotation UIs
- LFS support is awareness/detection baseline, not full workflow orchestration

## Troubleshooting
1. Run `scripts/check-deps.sh` to validate local tools.
2. Run targeted verify script for the active release sprint.
3. Inspect diagnostics panel for actionable blockers and slow operations.

## Support handoff
- Release verification entrypoint: `scripts/verify-sprint-24.sh`
- Local package output: `target/tmp/local-package` (or custom path)
- Escalation artifacts:
  - `docs/process/release_regression_matrix_sprint24.md`
  - `docs/process/rc_signoff_sprint24.md`

