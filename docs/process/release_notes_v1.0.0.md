# Branchforge v1.0.0 Release Notes

## Highlights
- Interactive conflict recovery and continuation flows
- Power staging with hunk stage/unstage/discard
- Repository productivity suite: stash, file history, blame baseline
- Advanced repo support baseline: worktrees, submodules, LFS awareness
- Plugin SDK hardening with manifest v1 and compatibility checks

## Quality and Hardening
- Beta diagnostics now include performance aggregates and blocker counters
- Keyboard and accessibility hints for core status/history actions
- Packaging smoke flow and release verification scripts for sprint 18-24

## Upgrade Notes
- Bundled plugins now report the package version from build metadata
- External plugins should use `plugin.json` manifest v1

## Verification
- Use `scripts/verify-sprint-24.sh` for RC/GA local release checklist

