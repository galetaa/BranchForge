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
        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_nanos())
            .unwrap_or(0);
        std::env::temp_dir().join(format!("branchforge-git-service-{nanos}"))
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
}
