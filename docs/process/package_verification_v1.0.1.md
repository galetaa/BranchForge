# Branchforge Package Verification v1.0.1

## Required Gate

Run the release verification command from the workspace root:

```sh
cargo run -p app_host -- --command "run release.verify"
```

For the beta channel wrapper:

```sh
./scripts/verify-beta-package.sh
```

This builds the local package, creates the release archive, signs the checksum manifest, verifies the signature, and recomputes checksums for packaged files.

## Package Contents

- `bin/app_host`
- bundled plugin executables under `plugins/`
- `manifest.json`
- `rollback.json`
- `sha256sums.txt`
- `sha256sums.sig`
- `sha256sums.pub`
- `release_notes.md`
- `docs/beta_user_guide.md`
- `docs/beta_known_issues.md`
- `docs/package_verification.md`

## Manual Spot Check

1. Unpack the archive in a clean directory.
2. Confirm `sha256sums.txt`, `sha256sums.sig`, and `sha256sums.pub` are present.
3. Start `./bin/app_host`.
4. Open a fixture repository and run `status.refresh`, `history.page`, `diff.worktree`, `auth.status`, `pr.list`, and `plugin.marketplace`.
