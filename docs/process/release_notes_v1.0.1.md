# Branchforge v1.0.1 Release Notes

## Highlights
- Interactive conflict recovery and continuation flows
- Power staging with hunk stage/unstage/discard
- Repository productivity suite: stash, file history, blame baseline
- Advanced repo support baseline: worktrees, submodules, LFS awareness
- Plugin SDK hardening with manifest v1 and compatibility checks
- Token-backed GitHub/GitLab PR listing, credential vault integration, and HTTPS credential seeding
- Marketplace plugin catalog refresh, update flow, signature verification, and sandbox risk labels
- Research-only virtual branch contexts for mapping worktree changes without checkout

## Quality and Hardening
- Beta diagnostics now include performance aggregates and blocker counters
- Keyboard and accessibility hints for core status/history actions
- Packaging smoke flow and runtime release verification entrypoints for sprint 18-24
- Public beta docs, known issues, and package verification guide are bundled into release packages

## Upgrade Notes
- Bundled plugins now report the package version from build metadata
- External plugins should use `plugin.json` manifest v1
- Signed community plugins should include `plugin.sig` and `plugin.pub` for `plugin.json` verification

## Verification
- Use `cargo run -p app_host -- --command "run release.verify"` for the public beta package checklist
