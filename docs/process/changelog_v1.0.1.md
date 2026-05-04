# Changelog v1.0.1

## Added
- Sprint 18: conflict session model and recovery operations
- Sprint 19: granular hunk workflows including discard hunk
- Sprint 20: stash/file-history/blame productivity baseline
- Sprint 21: worktree/submodule/LFS capability baseline
- Sprint 22: plugin manifest v1 and local lifecycle manager
- Sprint 23: beta hardening for diagnostics, keyboard and cache bounds
- Sprint 24: RC/GA release checklist, final docs and packaging checksums
- Collaboration layer: OS credential vault metadata, HTTPS token seeding, and live GitHub/GitLab PR list foundation
- Plugin ecosystem: marketplace catalog UX, update flow, signature verification, and sandbox classification
- Branch workflows: virtual branch research prototype for logical worktree contexts

## Changed
- Workspace version frozen to `1.0.1`
- Bundled plugin hello version derived from package version
- Diagnostics panel includes host/protocol version and performance aggregates
- Operational packaging, verification, and signing flows now run through `app_host` runtime commands instead of shell scripts
- Release packages now include beta user, known-issues, and package verification docs

## Fixed
- Stability fixes across release verify pipelines and regression gates
- Desktop status file buckets use distinct scroll IDs to avoid egui duplicate-id warnings
