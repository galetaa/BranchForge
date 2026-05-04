# Known Issues and Support Guide v1.0.1

## Known limitations
- `git-lfs` must be installed separately to use `diagnostics.lfs_status`, `diagnostics.lfs_fetch`, and `diagnostics.lfs_pull`
- Virtual branches are research-only and do not apply, hide, or shelve worktree files.
- Provider PR listing requires a stored token for the detected GitHub or GitLab host.
- Plugin signature verification requires `openssl` to be installed.

## Troubleshooting
1. Run `cargo run -p app_host -- --command "run ops.check_deps"` to validate local tools.
2. Run the targeted runtime verify command for the active release sprint.
3. Inspect diagnostics panel for actionable blockers, installed plugins, LFS status, and slow operations.
4. Use `run auth.status` and `run auth.seed_git <host> [username]` when HTTPS remotes fail.
5. Use `run plugin.marketplace` to validate registry wiring, package paths, signatures, and update metadata.
6. Use `conflict.list` followed by `conflict.focus <path>` when merge/rebase recovery needs a single-file diff.

## Support handoff
- Release verification entrypoint: `cargo run -p app_host -- --command "run release.verify"`
- Local package output: `target/tmp/local-package` (or custom path)
- Release archive entrypoint: `cargo run -p app_host -- --command "run release.package"`
- Signed artifact files:
  - `sha256sums.txt`
  - `sha256sums.sig`
  - `sha256sums.pub`
  - `signing.json`
- Escalation artifacts:
  - `docs/process/beta_user_guide_v1.0.1.md`
  - `docs/process/beta_known_issues_v1.0.1.md`
  - `docs/process/package_verification_v1.0.1.md`
  - `docs/process/release_regression_matrix_sprint24.md`
  - `docs/process/rc_signoff_sprint24.md`
