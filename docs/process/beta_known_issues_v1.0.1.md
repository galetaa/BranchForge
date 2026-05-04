# Branchforge Beta Known Issues v1.0.1

## Known Limitations

- Virtual branches are research-only. Switching a virtual branch changes UI context only and does not hide, shelve, or apply worktree files.
- Provider PR listing requires a stored GitHub or GitLab token for the detected host.
- Plugin signature verification requires `openssl` on the machine running the host.
- External plugin marketplace entries are local or URL-backed catalog records; remote package distribution policy is still conservative.
- Desktop packaging is validated as a local package layout. Native installer notarization is outside this beta build.

## Support Notes

- Run `run release.verify` before publishing a package.
- Run `run auth.status` when HTTPS operations fail.
- Run `run plugin.marketplace` to refresh plugin trust and update metadata.
- Use journal recovery commands before deleting local refs or cleaning the worktree externally.
