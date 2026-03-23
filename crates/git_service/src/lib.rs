use std::path::Path;
use std::process::Command;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GitCommand {
    pub program: String,
    pub args: Vec<String>,
}

impl GitCommand {
    pub fn status_porcelain() -> Self {
        Self {
            program: "git".to_string(),
            args: vec![
                "status".to_string(),
                "--porcelain=v2".to_string(),
                "-z".to_string(),
            ],
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GitRunResult {
    pub stdout: Vec<u8>,
    pub stderr: Vec<u8>,
    pub exit_code: i32,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RepoOpenResult {
    pub root: String,
    pub head: Option<String>,
    pub detached: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CommitSummary {
    pub oid: String,
    pub author: String,
    pub time: String,
    pub summary: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CommitDetails {
    pub oid: String,
    pub author: String,
    pub time: String,
    pub message: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct StatusSummary {
    pub staged: Vec<String>,
    pub unstaged: Vec<String>,
    pub untracked: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum GitServiceError {
    ProcessLaunch(String),
    GitError { exit_code: i32, stderr: String },
    Utf8Decode,
    ParseError(String),
}

pub fn run_git(cwd: &Path, args: &[&str]) -> Result<GitRunResult, GitServiceError> {
    let output = Command::new("git")
        .args(args)
        .current_dir(cwd)
        .output()
        .map_err(|err| GitServiceError::ProcessLaunch(err.to_string()))?;

    let exit_code = output.status.code().unwrap_or(-1);
    let result = GitRunResult {
        stdout: output.stdout,
        stderr: output.stderr,
        exit_code,
    };

    if exit_code != 0 {
        let stderr = String::from_utf8(result.stderr.clone())
            .unwrap_or_else(|_| "<non-utf8 stderr>".to_string());
        return Err(GitServiceError::GitError { exit_code, stderr });
    }

    Ok(result)
}

pub fn repo_open(cwd: &Path) -> Result<RepoOpenResult, GitServiceError> {
    let root_out = run_git(cwd, &["rev-parse", "--show-toplevel"])?;
    let head_out = run_git(cwd, &["branch", "--show-current"]);

    let root = String::from_utf8(root_out.stdout)
        .map_err(|_| GitServiceError::Utf8Decode)?
        .trim()
        .to_string();
    let head_raw = match head_out {
        Ok(out) => String::from_utf8(out.stdout)
            .map_err(|_| GitServiceError::Utf8Decode)?
            .trim()
            .to_string(),
        Err(_) => String::new(),
    };

    let detached = head_raw.is_empty();
    let head = if detached { None } else { Some(head_raw) };

    Ok(RepoOpenResult {
        root,
        head,
        detached,
    })
}

pub fn status_refresh(cwd: &Path) -> Result<StatusSummary, GitServiceError> {
    let out = run_git(cwd, &["status", "--porcelain=v2", "-z"])?;
    parse_status_porcelain_v2_z(&out.stdout)
}

pub fn stage_paths(cwd: &Path, paths: &[String]) -> Result<(), GitServiceError> {
    if paths.is_empty() {
        return Ok(());
    }

    let mut args = vec!["add".to_string(), "--".to_string()];
    args.extend(paths.iter().cloned());
    let refs: Vec<&str> = args.iter().map(String::as_str).collect();
    let _ = run_git(cwd, &refs)?;
    Ok(())
}

pub fn unstage_paths(cwd: &Path, paths: &[String]) -> Result<(), GitServiceError> {
    if paths.is_empty() {
        return Ok(());
    }

    let mut args = vec![
        "restore".to_string(),
        "--staged".to_string(),
        "--".to_string(),
    ];
    args.extend(paths.iter().cloned());
    let refs: Vec<&str> = args.iter().map(String::as_str).collect();
    match run_git(cwd, &refs) {
        Ok(_) => Ok(()),
        Err(_) => {
            // In repos without commits, restore --staged may fail; rm --cached keeps file in working tree.
            let mut fallback = vec!["rm".to_string(), "--cached".to_string(), "--".to_string()];
            fallback.extend(paths.iter().cloned());
            let fallback_refs: Vec<&str> = fallback.iter().map(String::as_str).collect();
            let _ = run_git(cwd, &fallback_refs)?;
            Ok(())
        }
    }
}

pub fn commit_create(cwd: &Path, message: &str) -> Result<(), GitServiceError> {
    let trimmed = message.trim();
    if trimmed.is_empty() {
        return Err(GitServiceError::ParseError(
            "commit message cannot be empty".to_string(),
        ));
    }

    let _ = run_git(cwd, &["commit", "-m", trimmed])?;
    Ok(())
}

pub fn commit_log_page(
    cwd: &Path,
    offset: usize,
    limit: usize,
) -> Result<Vec<CommitSummary>, GitServiceError> {
    let format = "--format=%H%x1f%an%x1f%ad%x1f%s";
    let max_count = limit.to_string();
    let skip = offset.to_string();
    let args = [
        "log",
        "--date=iso-strict",
        format,
        "--max-count",
        max_count.as_str(),
        "--skip",
        skip.as_str(),
    ];
    let out = run_git(cwd, &args)?;
    let text = String::from_utf8(out.stdout).map_err(|_| GitServiceError::Utf8Decode)?;
    let mut commits = Vec::new();

    for line in text.lines() {
        if line.trim().is_empty() {
            continue;
        }
        let parts: Vec<&str> = line.split('\x1f').collect();
        if parts.len() != 4 {
            return Err(GitServiceError::ParseError(
                "invalid commit log line".to_string(),
            ));
        }
        commits.push(CommitSummary {
            oid: parts[0].to_string(),
            author: parts[1].to_string(),
            time: parts[2].to_string(),
            summary: parts[3].to_string(),
        });
    }

    Ok(commits)
}

pub fn commit_details(cwd: &Path, oid: &str) -> Result<CommitDetails, GitServiceError> {
    let format = "--format=%H%x1f%an%x1f%ad%x1f%B";
    let args = ["show", "--quiet", "--date=iso-strict", format, oid];
    let out = run_git(cwd, &args)?;
    let text = String::from_utf8(out.stdout).map_err(|_| GitServiceError::Utf8Decode)?;
    let mut parts = text.splitn(4, '\x1f');
    let oid = parts
        .next()
        .ok_or_else(|| GitServiceError::ParseError("missing oid".to_string()))?
        .trim()
        .to_string();
    let author = parts
        .next()
        .ok_or_else(|| GitServiceError::ParseError("missing author".to_string()))?
        .trim()
        .to_string();
    let time = parts
        .next()
        .ok_or_else(|| GitServiceError::ParseError("missing time".to_string()))?
        .trim()
        .to_string();
    let message = parts
        .next()
        .ok_or_else(|| GitServiceError::ParseError("missing message".to_string()))?
        .trim_end()
        .to_string();

    Ok(CommitDetails {
        oid,
        author,
        time,
        message,
    })
}

pub fn diff_worktree(
    cwd: &Path,
    paths: &[String],
    max_bytes: usize,
) -> Result<String, GitServiceError> {
    let mut args = vec!["diff".to_string(), "--patch".to_string()];
    if !paths.is_empty() {
        args.push("--".to_string());
        args.extend(paths.iter().cloned());
    }
    let refs: Vec<&str> = args.iter().map(String::as_str).collect();
    let out = run_git(cwd, &refs)?;
    let mut text = String::from_utf8(out.stdout).map_err(|_| GitServiceError::Utf8Decode)?;
    if text.len() > max_bytes {
        text.truncate(max_bytes);
        text.push_str("\n... diff truncated ...\n");
    }
    Ok(text)
}

pub fn diff_commit(cwd: &Path, oid: &str, max_bytes: usize) -> Result<String, GitServiceError> {
    let out = run_git(cwd, &["show", "--patch", "--format=short", oid])?;
    let mut text = String::from_utf8(out.stdout).map_err(|_| GitServiceError::Utf8Decode)?;
    if text.len() > max_bytes {
        text.truncate(max_bytes);
        text.push_str("\n... diff truncated ...\n");
    }
    Ok(text)
}

pub fn parse_status_porcelain_v2_z(raw: &[u8]) -> Result<StatusSummary, GitServiceError> {
    let mut summary = StatusSummary::default();
    let tokens: Vec<&[u8]> = raw.split(|b| *b == 0).filter(|t| !t.is_empty()).collect();
    let mut idx = 0;

    while idx < tokens.len() {
        let token = tokens[idx];
        let line = std::str::from_utf8(token).map_err(|_| GitServiceError::Utf8Decode)?;

        if let Some(rest) = line.strip_prefix("? ") {
            summary.untracked.push(rest.to_string());
            idx += 1;
            continue;
        }

        if let Some(_rest) = line.strip_prefix("! ") {
            idx += 1;
            continue;
        }

        let mut parts = line.split_whitespace();
        let kind = parts
            .next()
            .ok_or_else(|| GitServiceError::ParseError("missing record kind".to_string()))?;

        match kind {
            "1" | "2" => {
                let xy = parts
                    .next()
                    .ok_or_else(|| GitServiceError::ParseError("missing XY status".to_string()))?;
                let x = xy.chars().next().unwrap_or('.');
                let y = xy.chars().nth(1).unwrap_or('.');
                let path = parts
                    .last()
                    .ok_or_else(|| {
                        GitServiceError::ParseError("missing path in status entry".to_string())
                    })?
                    .to_string();

                if x != '.' {
                    summary.staged.push(path.clone());
                }
                if y != '.' {
                    summary.unstaged.push(path);
                }

                idx += 1;
                if kind == "2" {
                    // Rename/copy records have an additional NUL-terminated orig-path field.
                    idx += 1;
                }
            }
            other => {
                return Err(GitServiceError::ParseError(format!(
                    "unsupported status record kind: {other}"
                )));
            }
        }
    }

    Ok(summary)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn unique_temp_dir() -> std::path::PathBuf {
        static COUNTER: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_nanos())
            .unwrap_or(0);
        let seq = COUNTER.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        std::env::temp_dir().join(format!("branchforge-git-service-{nanos}-{seq}"))
    }

    #[test]
    fn builds_status_command() {
        let cmd = GitCommand::status_porcelain();
        assert_eq!(cmd.program, "git");
        assert_eq!(cmd.args.len(), 3);
    }

    #[test]
    fn parses_status_porcelain_fixture() {
        let fixture = b"1 M. N... 100644 100644 100644 abcdef abcdef src/lib.rs\0? notes.txt\0";
        let parsed = parse_status_porcelain_v2_z(fixture);
        assert!(parsed.is_ok());

        if let Ok(summary) = parsed {
            assert_eq!(summary.staged, vec!["src/lib.rs".to_string()]);
            assert_eq!(summary.untracked, vec!["notes.txt".to_string()]);
        }
    }

    #[test]
    fn parses_status_porcelain_rename_record() {
        let fixture = b"2 R. N... 100644 100644 100644 abcdef abcdef new_name.txt\0old_name.txt\0";
        let parsed = parse_status_porcelain_v2_z(fixture);
        assert!(parsed.is_ok());

        if let Ok(summary) = parsed {
            assert_eq!(summary.staged, vec!["new_name.txt".to_string()]);
            assert!(summary.unstaged.is_empty());
        }
    }

    #[test]
    fn parses_status_porcelain_copy_record() {
        let fixture = b"2 C. N... 100644 100644 100644 abcdef abcdef copy.txt\0orig.txt\0";
        let parsed = parse_status_porcelain_v2_z(fixture);
        assert!(parsed.is_ok());

        if let Ok(summary) = parsed {
            assert_eq!(summary.staged, vec!["copy.txt".to_string()]);
            assert!(summary.unstaged.is_empty());
        }
    }

    #[test]
    fn parse_rejects_unsupported_status_kind() {
        let fixture = b"u UU N... 100644 100644 100644 abcdef abcdef conflict.txt\0";
        let parsed = parse_status_porcelain_v2_z(fixture);
        assert!(parsed.is_err());
    }

    #[test]
    fn run_git_executes_without_shell() {
        let cwd = std::env::current_dir();
        assert!(cwd.is_ok());

        if let Ok(path) = cwd {
            let result = run_git(&path, &["--version"]);
            assert!(result.is_ok());
        }
    }

    #[test]
    fn repo_open_and_status_refresh_work_on_temp_repo() {
        let repo_dir = unique_temp_dir();
        let create_dir = std::fs::create_dir_all(&repo_dir);
        assert!(create_dir.is_ok());

        let init = run_git(&repo_dir, &["init"]);
        assert!(init.is_ok());

        let write = std::fs::write(repo_dir.join("README.md"), "hello\n");
        assert!(write.is_ok());

        let open = repo_open(&repo_dir);
        assert!(open.is_ok());

        let status = status_refresh(&repo_dir);
        assert!(status.is_ok());
        if let Ok(summary) = status {
            assert!(summary.untracked.iter().any(|p| p == "README.md"));
        }

        let _ = std::fs::remove_dir_all(&repo_dir);
    }

    #[test]
    fn repo_open_fails_for_non_repo() {
        let repo_dir = unique_temp_dir();
        assert!(std::fs::create_dir_all(&repo_dir).is_ok());
        let result = repo_open(&repo_dir);
        assert!(matches!(result, Err(GitServiceError::GitError { .. })));
        let _ = std::fs::remove_dir_all(&repo_dir);
    }

    #[test]
    fn stage_paths_fails_for_missing_file() {
        let repo_dir = unique_temp_dir();
        assert!(std::fs::create_dir_all(&repo_dir).is_ok());
        assert!(run_git(&repo_dir, &["init"]).is_ok());

        let result = stage_paths(&repo_dir, &["missing.txt".to_string()]);
        assert!(matches!(result, Err(GitServiceError::GitError { .. })));

        let _ = std::fs::remove_dir_all(&repo_dir);
    }

    #[test]
    fn stage_then_unstage_moves_file_between_groups() {
        let repo_dir = unique_temp_dir();
        assert!(std::fs::create_dir_all(&repo_dir).is_ok());
        assert!(run_git(&repo_dir, &["init"]).is_ok());

        let file = "README.md".to_string();
        assert!(std::fs::write(repo_dir.join(&file), "hello\n").is_ok());

        assert!(stage_paths(&repo_dir, std::slice::from_ref(&file)).is_ok());
        let staged = status_refresh(&repo_dir);
        assert!(staged.is_ok());
        if let Ok(summary) = staged {
            assert!(summary.staged.iter().any(|p| p == &file));
        }

        assert!(unstage_paths(&repo_dir, std::slice::from_ref(&file)).is_ok());
        let unstaged = status_refresh(&repo_dir);
        assert!(unstaged.is_ok());
        if let Ok(summary) = unstaged {
            assert!(summary.untracked.iter().any(|p| p == &file));
        }

        let _ = std::fs::remove_dir_all(&repo_dir);
    }

    #[test]
    fn stage_paths_handles_multiple_files() {
        let repo_dir = unique_temp_dir();
        assert!(std::fs::create_dir_all(&repo_dir).is_ok());
        assert!(run_git(&repo_dir, &["init"]).is_ok());

        let first = "first.txt".to_string();
        let second = "second.txt".to_string();
        assert!(std::fs::write(repo_dir.join(&first), "one\n").is_ok());
        assert!(std::fs::write(repo_dir.join(&second), "two\n").is_ok());

        assert!(stage_paths(&repo_dir, &[first.clone(), second.clone()]).is_ok());
        let status = status_refresh(&repo_dir);
        assert!(status.is_ok());
        if let Ok(summary) = status {
            assert!(summary.staged.iter().any(|p| p == &first));
            assert!(summary.staged.iter().any(|p| p == &second));
        }

        let _ = std::fs::remove_dir_all(&repo_dir);
    }

    #[test]
    fn commit_create_rejects_empty_message() {
        let repo_dir = unique_temp_dir();
        assert!(std::fs::create_dir_all(&repo_dir).is_ok());
        assert!(run_git(&repo_dir, &["init"]).is_ok());
        assert!(run_git(&repo_dir, &["config", "user.email", "dev@example.com"]).is_ok());
        assert!(run_git(&repo_dir, &["config", "user.name", "Dev User"]).is_ok());

        let result = commit_create(&repo_dir, "   ");
        assert!(matches!(result, Err(GitServiceError::ParseError(_))));

        let _ = std::fs::remove_dir_all(&repo_dir);
    }

    #[test]
    fn commit_create_fails_without_staged_changes() {
        let repo_dir = unique_temp_dir();
        assert!(std::fs::create_dir_all(&repo_dir).is_ok());
        assert!(run_git(&repo_dir, &["init"]).is_ok());
        assert!(run_git(&repo_dir, &["config", "user.email", "dev@example.com"]).is_ok());
        assert!(run_git(&repo_dir, &["config", "user.name", "Dev User"]).is_ok());
        assert!(std::fs::write(repo_dir.join("README.md"), "hello\n").is_ok());

        let result = commit_create(&repo_dir, "Initial commit");
        assert!(matches!(result, Err(GitServiceError::GitError { .. })));

        let _ = std::fs::remove_dir_all(&repo_dir);
    }

    #[test]
    fn commit_create_creates_commit_for_staged_file() {
        let repo_dir = unique_temp_dir();
        assert!(std::fs::create_dir_all(&repo_dir).is_ok());
        assert!(run_git(&repo_dir, &["init"]).is_ok());
        assert!(run_git(&repo_dir, &["config", "user.email", "dev@example.com"]).is_ok());
        assert!(run_git(&repo_dir, &["config", "user.name", "Dev User"]).is_ok());

        let file = "README.md".to_string();
        assert!(std::fs::write(repo_dir.join(&file), "hello\n").is_ok());
        assert!(stage_paths(&repo_dir, std::slice::from_ref(&file)).is_ok());
        assert!(commit_create(&repo_dir, "Initial commit").is_ok());

        let status = status_refresh(&repo_dir);
        assert!(status.is_ok());
        if let Ok(summary) = status {
            assert!(summary.staged.is_empty());
        }

        let _ = std::fs::remove_dir_all(&repo_dir);
    }

    #[test]
    fn commit_log_page_returns_entries() {
        let repo_dir = unique_temp_dir();
        assert!(std::fs::create_dir_all(&repo_dir).is_ok());
        assert!(run_git(&repo_dir, &["init"]).is_ok());
        assert!(run_git(&repo_dir, &["config", "user.email", "dev@example.com"]).is_ok());
        assert!(run_git(&repo_dir, &["config", "user.name", "Dev User"]).is_ok());

        assert!(std::fs::write(repo_dir.join("one.txt"), "one\n").is_ok());
        assert!(stage_paths(&repo_dir, &["one.txt".to_string()]).is_ok());
        assert!(commit_create(&repo_dir, "commit one").is_ok());

        assert!(std::fs::write(repo_dir.join("two.txt"), "two\n").is_ok());
        assert!(stage_paths(&repo_dir, &["two.txt".to_string()]).is_ok());
        assert!(commit_create(&repo_dir, "commit two").is_ok());

        let page = commit_log_page(&repo_dir, 0, 1);
        assert!(page.is_ok());
        if let Ok(commits) = page {
            assert_eq!(commits.len(), 1);
            assert!(commits[0].summary.contains("commit"));
        }

        let _ = std::fs::remove_dir_all(&repo_dir);
    }

    #[test]
    fn diff_worktree_and_commit_render() {
        let repo_dir = unique_temp_dir();
        assert!(std::fs::create_dir_all(&repo_dir).is_ok());
        assert!(run_git(&repo_dir, &["init"]).is_ok());
        assert!(run_git(&repo_dir, &["config", "user.email", "dev@example.com"]).is_ok());
        assert!(run_git(&repo_dir, &["config", "user.name", "Dev User"]).is_ok());

        let file = "diff.txt".to_string();
        assert!(std::fs::write(repo_dir.join(&file), "line 1\n").is_ok());
        assert!(stage_paths(&repo_dir, std::slice::from_ref(&file)).is_ok());
        assert!(commit_create(&repo_dir, "commit diff").is_ok());

        assert!(std::fs::write(repo_dir.join(&file), "line 1\nline 2\n").is_ok());
        let worktree = diff_worktree(&repo_dir, &[], 10_000);
        assert!(worktree.is_ok());
        if let Ok(diff) = worktree {
            assert!(diff.contains("diff --git"));
        }

        let commits = commit_log_page(&repo_dir, 0, 1).expect("page");
        let oid = commits[0].oid.clone();
        let commit_diff = diff_commit(&repo_dir, &oid, 10_000);
        assert!(commit_diff.is_ok());
        if let Ok(diff) = commit_diff {
            assert!(diff.contains("commit"));
        }

        let _ = std::fs::remove_dir_all(&repo_dir);
    }
}
