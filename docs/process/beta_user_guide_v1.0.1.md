# Branchforge Beta User Guide v1.0.1

## Install And Launch

1. Build or unpack the beta package for your platform.
2. Start the host with `./bin/app_host` from the package root, or run `cargo run -p app_host` from the workspace.
3. Keep the bundled `plugins/` directory beside the host binary.

## First Repository

1. Open a repository with `open /path/to/repo`.
2. Refresh repository state with `run status.refresh`.
3. Use `run diff.worktree <path>` or `run diff.index <path>` to inspect changes.

## Daily Workflow

- Stage files with `run index.stage_paths <path...>`.
- Create commits with `run commit.create`.
- Review history with `run history.page 0 50`.
- Manage branches with `run branch.create`, `run branch.checkout`, and `run branch.delete`.
- Use `run stack.detect` and `run virtual.detect` for advanced branch experiments.

## Remote And PR Workflow

- Store provider tokens with `run auth.login <host> <username> <token> [provider]`.
- Seed Git HTTPS credentials with `run auth.seed_git <host> [username]`.
- List provider PRs with `run pr.list`.
- Push and pull through the remote commands after reviewing previews for destructive flows.

## Recovery

Branchforge records destructive operations in the journal. Use `run diagnostics.journal_summary`, `run journal.open_entry`, and recovery commands before using external reset tools.
