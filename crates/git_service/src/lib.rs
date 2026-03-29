use std::collections::BTreeSet;
use std::path::Path;
use std::process::Command;
use std::process::Stdio;

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
    pub conflict_state: Option<ConflictState>,
    pub capabilities: RepoCapabilities,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CommitSummary {
    pub oid: String,
    pub author: String,
    pub time: String,
    pub summary: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StashEntry {
    pub reference: String,
    pub message: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BlameLine {
    pub line_no: usize,
    pub oid: String,
    pub author: String,
    pub content: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct RepoCapabilities {
    pub is_linked_worktree: bool,
    pub has_submodules: bool,
    pub lfs_detected: bool,
    pub lfs_available: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WorktreeEntry {
    pub path: String,
    pub head: Option<String>,
    pub branch: Option<String>,
    pub bare: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SubmoduleEntry {
    pub path: String,
    pub oid: String,
    pub status: char,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CompareSummary {
    pub base_ref: String,
    pub head_ref: String,
    pub ahead: usize,
    pub behind: usize,
    pub commits: Vec<CommitSummary>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MergeMode {
    FastForward,
    NoFastForward,
    Squash,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ResetMode {
    Soft,
    Mixed,
    Hard,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RebaseAction {
    Pick,
    Reword,
    Edit,
    Squash,
    Fixup,
    Drop,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RebasePlanEntry {
    pub oid: String,
    pub summary: String,
    pub action: RebaseAction,
    pub warnings: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RebasePlan {
    pub base_ref: String,
    pub base_oid: Option<String>,
    pub entries: Vec<RebasePlanEntry>,
    pub autosquash_aware: bool,
    pub published_history_warning: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RebaseSessionHook {
    pub active: bool,
    pub current_step: Option<usize>,
    pub total_steps: Option<usize>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TagTarget {
    pub name: String,
    pub oid: String,
    pub object_type: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ConflictState {
    Merge,
    Rebase,
    CherryPick,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ConflictChoice {
    Ours,
    Theirs,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ConflictFile {
    pub path: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ConflictSessionSnapshot {
    pub state: ConflictState,
    pub files: Vec<ConflictFile>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CommitDetails {
    pub oid: String,
    pub author: String,
    pub time: String,
    pub message: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BranchInfo {
    pub name: String,
    pub is_current: bool,
    pub upstream: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct StatusSummary {
    pub staged: Vec<String>,
    pub unstaged: Vec<String>,
    pub untracked: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DiffHunk {
    pub file_path: String,
    pub hunk_index: usize,
    pub header: String,
    pub lines: Vec<String>,
    pub patch: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DiffOutput {
    pub text: String,
    pub hunks: Vec<DiffHunk>,
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

pub fn run_git_with_input(
    cwd: &Path,
    args: &[&str],
    input: &[u8],
) -> Result<GitRunResult, GitServiceError> {
    let mut child = Command::new("git")
        .args(args)
        .current_dir(cwd)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|err| GitServiceError::ProcessLaunch(err.to_string()))?;

    if let Some(mut stdin) = child.stdin.take() {
        std::io::Write::write_all(&mut stdin, input)
            .map_err(|err| GitServiceError::ProcessLaunch(err.to_string()))?;
    }

    let output = child
        .wait_with_output()
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
    let conflict_state = detect_conflict_state(cwd)?;
    let capabilities = repo_capabilities(cwd)?;

    Ok(RepoOpenResult {
        root,
        head,
        detached,
        conflict_state,
        capabilities,
    })
}

pub fn repo_capabilities(cwd: &Path) -> Result<RepoCapabilities, GitServiceError> {
    let common = run_git(cwd, &["rev-parse", "--git-common-dir"])?;
    let git_dir = run_git(cwd, &["rev-parse", "--git-dir"])?;
    let common = String::from_utf8(common.stdout)
        .map_err(|_| GitServiceError::Utf8Decode)?
        .trim()
        .to_string();
    let git_dir = String::from_utf8(git_dir.stdout)
        .map_err(|_| GitServiceError::Utf8Decode)?
        .trim()
        .to_string();

    let submodules = list_submodules(cwd)?;
    let lfs_available = is_lfs_available(cwd);
    let lfs_detected = detect_lfs_baseline(cwd);

    Ok(RepoCapabilities {
        is_linked_worktree: common != git_dir,
        has_submodules: !submodules.is_empty(),
        lfs_detected,
        lfs_available,
    })
}

pub fn lfs_status(cwd: &Path) -> Result<String, GitServiceError> {
    if !is_lfs_available(cwd) {
        return Ok(lfs_unavailable_message("inspect LFS state"));
    }
    let out = run_git(cwd, &["lfs", "status"])?;
    String::from_utf8(out.stdout).map_err(|_| GitServiceError::Utf8Decode)
}

pub fn lfs_fetch(cwd: &Path) -> Result<String, GitServiceError> {
    if !is_lfs_available(cwd) {
        return Ok(lfs_unavailable_message("fetch LFS objects"));
    }
    let out = run_git(cwd, &["lfs", "fetch"])?;
    let stdout = String::from_utf8(out.stdout).map_err(|_| GitServiceError::Utf8Decode)?;
    let stderr = String::from_utf8(out.stderr).map_err(|_| GitServiceError::Utf8Decode)?;
    Ok(format!("{}{}", stdout, stderr).trim().to_string())
}

pub fn lfs_pull(cwd: &Path) -> Result<String, GitServiceError> {
    if !is_lfs_available(cwd) {
        return Ok(lfs_unavailable_message("pull LFS objects"));
    }
    let out = run_git(cwd, &["lfs", "pull"])?;
    let stdout = String::from_utf8(out.stdout).map_err(|_| GitServiceError::Utf8Decode)?;
    let stderr = String::from_utf8(out.stderr).map_err(|_| GitServiceError::Utf8Decode)?;
    Ok(format!("{}{}", stdout, stderr).trim().to_string())
}

pub fn list_worktrees(cwd: &Path) -> Result<Vec<WorktreeEntry>, GitServiceError> {
    let out = run_git(cwd, &["worktree", "list", "--porcelain"])?;
    let text = String::from_utf8(out.stdout).map_err(|_| GitServiceError::Utf8Decode)?;

    let mut entries = Vec::new();
    let mut current_path: Option<String> = None;
    let mut current_head: Option<String> = None;
    let mut current_branch: Option<String> = None;
    let mut current_bare = false;

    for line in text.lines() {
        let trimmed = line.trim();
        if trimmed.is_empty() {
            if let Some(path) = current_path.take() {
                entries.push(WorktreeEntry {
                    path,
                    head: current_head.take(),
                    branch: current_branch.take(),
                    bare: current_bare,
                });
            }
            current_bare = false;
            continue;
        }
        if let Some(value) = trimmed.strip_prefix("worktree ") {
            current_path = Some(value.to_string());
            continue;
        }
        if let Some(value) = trimmed.strip_prefix("HEAD ") {
            current_head = Some(value.to_string());
            continue;
        }
        if let Some(value) = trimmed.strip_prefix("branch ") {
            current_branch = Some(value.to_string());
            continue;
        }
        if trimmed == "bare" {
            current_bare = true;
        }
    }

    if let Some(path) = current_path.take() {
        entries.push(WorktreeEntry {
            path,
            head: current_head.take(),
            branch: current_branch.take(),
            bare: current_bare,
        });
    }

    Ok(entries)
}

pub fn worktree_add(cwd: &Path, path: &Path, branch: Option<&str>) -> Result<(), GitServiceError> {
    let path = path
        .to_str()
        .ok_or_else(|| GitServiceError::ParseError("invalid worktree path".to_string()))?;
    match branch {
        Some(branch) if !branch.trim().is_empty() => {
            let _ = run_git(cwd, &["worktree", "add", "-b", branch, path])?;
        }
        _ => {
            let _ = run_git(cwd, &["worktree", "add", path])?;
        }
    }
    Ok(())
}

pub fn worktree_remove(cwd: &Path, path: &Path, force: bool) -> Result<(), GitServiceError> {
    let path = path
        .to_str()
        .ok_or_else(|| GitServiceError::ParseError("invalid worktree path".to_string()))?;
    if force {
        let _ = run_git(cwd, &["worktree", "remove", "--force", path])?;
    } else {
        let _ = run_git(cwd, &["worktree", "remove", path])?;
    }
    Ok(())
}

pub fn list_submodules(cwd: &Path) -> Result<Vec<SubmoduleEntry>, GitServiceError> {
    let out = run_git(cwd, &["submodule", "status"])?;
    let text = String::from_utf8(out.stdout).map_err(|_| GitServiceError::Utf8Decode)?;
    let mut entries = Vec::new();
    for line in text.lines() {
        let trimmed = line.trim();
        if trimmed.is_empty() {
            continue;
        }
        let status = trimmed.chars().next().unwrap_or(' ');
        let rest = trimmed.trim_start_matches([' ', '-', '+', 'U']);
        let mut parts = rest.split_whitespace();
        let oid = parts.next().unwrap_or_default().to_string();
        let path = parts.next().unwrap_or_default().to_string();
        if !path.is_empty() {
            entries.push(SubmoduleEntry { path, oid, status });
        }
    }
    Ok(entries)
}

pub fn submodule_init_update(cwd: &Path, path: Option<&str>) -> Result<(), GitServiceError> {
    match path {
        Some(path) if !path.trim().is_empty() => {
            let _ = run_git(cwd, &["submodule", "update", "--init", "--", path])?;
        }
        _ => {
            let _ = run_git(cwd, &["submodule", "update", "--init", "--recursive"])?;
        }
    }
    Ok(())
}

fn detect_lfs_baseline(cwd: &Path) -> bool {
    if is_lfs_available(cwd) {
        return true;
    }
    let attrs = cwd.join(".gitattributes");
    std::fs::read_to_string(attrs)
        .ok()
        .map(|text| text.contains("filter=lfs"))
        .unwrap_or(false)
}

fn is_lfs_available(cwd: &Path) -> bool {
    run_git(cwd, &["lfs", "version"]).is_ok()
}

fn lfs_unavailable_message(action: &str) -> String {
    format!("git-lfs is not installed on this machine. Install git-lfs to {action}.")
}

pub fn detect_conflict_state(cwd: &Path) -> Result<Option<ConflictState>, GitServiceError> {
    let rebase_apply = git_path(cwd, "rebase-apply")?;
    if rebase_apply.exists() {
        return Ok(Some(ConflictState::Rebase));
    }
    let rebase_merge = git_path(cwd, "rebase-merge")?;
    if rebase_merge.exists() {
        return Ok(Some(ConflictState::Rebase));
    }
    let merge_head = git_path(cwd, "MERGE_HEAD")?;
    if merge_head.exists() {
        return Ok(Some(ConflictState::Merge));
    }
    let cherry_pick = git_path(cwd, "CHERRY_PICK_HEAD")?;
    if cherry_pick.exists() {
        return Ok(Some(ConflictState::CherryPick));
    }
    Ok(None)
}

fn git_path(cwd: &Path, sub_path: &str) -> Result<std::path::PathBuf, GitServiceError> {
    let out = run_git(cwd, &["rev-parse", "--git-path", sub_path])?;
    let raw = String::from_utf8(out.stdout).map_err(|_| GitServiceError::Utf8Decode)?;
    let candidate = std::path::PathBuf::from(raw.trim());
    if candidate.is_absolute() {
        Ok(candidate)
    } else {
        Ok(cwd.join(candidate))
    }
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
    commit_log_page_filtered(cwd, offset, limit, None, None)
}

pub fn commit_log_page_filtered(
    cwd: &Path,
    offset: usize,
    limit: usize,
    author: Option<&str>,
    text: Option<&str>,
) -> Result<Vec<CommitSummary>, GitServiceError> {
    commit_log_page_filtered_with_hash_prefix(cwd, offset, limit, author, text, None)
}

pub fn commit_log_page_filtered_with_hash_prefix(
    cwd: &Path,
    offset: usize,
    limit: usize,
    author: Option<&str>,
    text: Option<&str>,
    hash_prefix: Option<&str>,
) -> Result<Vec<CommitSummary>, GitServiceError> {
    let format = "--format=%H%x1f%an%x1f%ad%x1f%s";
    let mut args = vec![
        "log".to_string(),
        "--date=iso-strict".to_string(),
        format.to_string(),
    ];

    if hash_prefix.is_none() {
        args.push("--max-count".to_string());
        args.push(limit.to_string());
        args.push("--skip".to_string());
        args.push(offset.to_string());
    }
    if let Some(author) = author {
        args.push(format!("--author={author}"));
    }
    if let Some(text) = text {
        args.push(format!("--grep={text}"));
    }
    let refs: Vec<&str> = args.iter().map(String::as_str).collect();
    let out = run_git(cwd, &refs)?;
    let text = String::from_utf8(out.stdout).map_err(|_| GitServiceError::Utf8Decode)?;
    let mut commits = Vec::new();

    let mut all = Vec::new();
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
        all.push(CommitSummary {
            oid: parts[0].to_string(),
            author: parts[1].to_string(),
            time: parts[2].to_string(),
            summary: parts[3].to_string(),
        });
    }

    if let Some(prefix) = hash_prefix {
        let filtered = all
            .into_iter()
            .filter(|item| item.oid.starts_with(prefix))
            .skip(offset)
            .take(limit)
            .collect();
        return Ok(filtered);
    }

    commits.extend(all);

    Ok(commits)
}

pub fn stash_create(cwd: &Path, message: Option<&str>) -> Result<(), GitServiceError> {
    let mut args = vec!["stash", "push", "--include-untracked"];
    if let Some(message) = message {
        let trimmed = message.trim();
        if !trimmed.is_empty() {
            args.push("-m");
            args.push(trimmed);
        }
    }
    let _ = run_git(cwd, &args)?;
    Ok(())
}

pub fn stash_list(cwd: &Path) -> Result<Vec<StashEntry>, GitServiceError> {
    let out = run_git(cwd, &["stash", "list", "--format=%gd%x1f%gs"])?;
    let text = String::from_utf8(out.stdout).map_err(|_| GitServiceError::Utf8Decode)?;
    let mut entries = Vec::new();
    for line in text.lines() {
        if line.trim().is_empty() {
            continue;
        }
        let mut parts = line.splitn(2, '\x1f');
        let reference = parts.next().unwrap_or_default().trim().to_string();
        let message = parts.next().unwrap_or_default().trim().to_string();
        if !reference.is_empty() {
            entries.push(StashEntry { reference, message });
        }
    }
    Ok(entries)
}

pub fn stash_apply(cwd: &Path, reference: &str) -> Result<(), GitServiceError> {
    let _ = run_git(cwd, &["stash", "apply", reference])?;
    Ok(())
}

pub fn stash_pop(cwd: &Path, reference: Option<&str>) -> Result<(), GitServiceError> {
    match reference {
        Some(reference) if !reference.trim().is_empty() => {
            let _ = run_git(cwd, &["stash", "pop", reference])?;
        }
        _ => {
            let _ = run_git(cwd, &["stash", "pop"])?;
        }
    }
    Ok(())
}

pub fn stash_drop(cwd: &Path, reference: &str) -> Result<(), GitServiceError> {
    let _ = run_git(cwd, &["stash", "drop", reference])?;
    Ok(())
}

pub fn file_history_page(
    cwd: &Path,
    path: &str,
    offset: usize,
    limit: usize,
) -> Result<Vec<CommitSummary>, GitServiceError> {
    let format = "--format=%H%x1f%an%x1f%ad%x1f%s";
    let out = run_git(
        cwd,
        &[
            "log",
            "--follow",
            "--date=iso-strict",
            format,
            "--max-count",
            &limit.to_string(),
            "--skip",
            &offset.to_string(),
            "--",
            path,
        ],
    )?;
    let text = String::from_utf8(out.stdout).map_err(|_| GitServiceError::Utf8Decode)?;
    let mut commits = Vec::new();
    for line in text.lines() {
        if line.trim().is_empty() {
            continue;
        }
        let parts: Vec<&str> = line.split('\x1f').collect();
        if parts.len() != 4 {
            return Err(GitServiceError::ParseError(
                "invalid file history log line".to_string(),
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

pub fn file_blame(
    cwd: &Path,
    path: &str,
    rev: Option<&str>,
) -> Result<Vec<BlameLine>, GitServiceError> {
    let mut args = vec!["blame", "--line-porcelain"];
    if let Some(rev) = rev
        && !rev.trim().is_empty()
    {
        args.push(rev);
    }
    args.push("--");
    args.push(path);
    let out = run_git(cwd, &args)?;
    let text = String::from_utf8(out.stdout).map_err(|_| GitServiceError::Utf8Decode)?;

    let mut lines = Vec::new();
    let mut current_oid = String::new();
    let mut current_author = String::new();

    for line in text.lines() {
        if line.is_empty() {
            continue;
        }

        if let Some(content) = line.strip_prefix('\t') {
            let line_no = lines.len() + 1;
            lines.push(BlameLine {
                line_no,
                oid: current_oid.clone(),
                author: current_author.clone(),
                content: content.to_string(),
            });
            continue;
        }

        if let Some(author) = line.strip_prefix("author ") {
            current_author = author.to_string();
            continue;
        }

        let mut parts = line.split_whitespace();
        if let Some(first) = parts.next()
            && first.len() >= 8
            && first.chars().all(|ch| ch.is_ascii_hexdigit())
        {
            current_oid = first.to_string();
        }
    }

    Ok(lines)
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
    let text = diff_worktree_raw(cwd, paths)?;
    Ok(truncate_diff(text, max_bytes))
}

pub fn diff_worktree_with_hunks(
    cwd: &Path,
    paths: &[String],
    max_bytes: usize,
) -> Result<DiffOutput, GitServiceError> {
    let text = diff_worktree_raw(cwd, paths)?;
    let hunks = parse_unified_diff_hunks(&text);
    let truncated = truncate_diff(text, max_bytes);
    Ok(DiffOutput {
        text: truncated,
        hunks,
    })
}

pub fn diff_index_with_hunks(
    cwd: &Path,
    paths: &[String],
    max_bytes: usize,
) -> Result<DiffOutput, GitServiceError> {
    let text = diff_index_raw(cwd, paths)?;
    let hunks = parse_unified_diff_hunks(&text);
    let truncated = truncate_diff(text, max_bytes);
    Ok(DiffOutput {
        text: truncated,
        hunks,
    })
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

pub fn diff_compare_with_hunks(
    cwd: &Path,
    base_ref: &str,
    head_ref: &str,
    max_bytes: usize,
) -> Result<DiffOutput, GitServiceError> {
    let text = diff_between_refs_raw(cwd, base_ref, head_ref)?;
    let hunks = parse_unified_diff_hunks(&text);
    let truncated = truncate_diff(text, max_bytes);
    Ok(DiffOutput {
        text: truncated,
        hunks,
    })
}

pub fn compare_refs(
    cwd: &Path,
    base_ref: &str,
    head_ref: &str,
    max_commits: usize,
) -> Result<CompareSummary, GitServiceError> {
    let ahead = count_revisions(cwd, &format!("{base_ref}..{head_ref}"))?;
    let behind = count_revisions(cwd, &format!("{head_ref}..{base_ref}"))?;
    let commits = commit_log_range(cwd, base_ref, head_ref, max_commits)?;
    Ok(CompareSummary {
        base_ref: base_ref.to_string(),
        head_ref: head_ref.to_string(),
        ahead,
        behind,
        commits,
    })
}

pub fn merge_ref(cwd: &Path, source_ref: &str, mode: MergeMode) -> Result<(), GitServiceError> {
    let mut args = vec!["merge".to_string()];
    match mode {
        MergeMode::FastForward => args.push("--ff-only".to_string()),
        MergeMode::NoFastForward => args.push("--no-ff".to_string()),
        MergeMode::Squash => args.push("--squash".to_string()),
    }
    args.push(source_ref.to_string());
    let refs: Vec<&str> = args.iter().map(String::as_str).collect();
    let _ = run_git(cwd, &refs)?;
    Ok(())
}

pub fn merge_abort(cwd: &Path) -> Result<(), GitServiceError> {
    let _ = run_git(cwd, &["merge", "--abort"])?;
    Ok(())
}

pub fn merge_continue(cwd: &Path) -> Result<(), GitServiceError> {
    continue_with_editor_fallback(cwd, &["merge", "--continue"])
}

pub fn cherry_pick_commit(cwd: &Path, oid: &str) -> Result<(), GitServiceError> {
    let _ = run_git(cwd, &["cherry-pick", oid])?;
    Ok(())
}

pub fn cherry_pick_abort(cwd: &Path) -> Result<(), GitServiceError> {
    let _ = run_git(cwd, &["cherry-pick", "--abort"])?;
    Ok(())
}

pub fn cherry_pick_continue(cwd: &Path) -> Result<(), GitServiceError> {
    continue_with_editor_fallback(cwd, &["cherry-pick", "--continue"])
}

pub fn list_conflicted_files(cwd: &Path) -> Result<Vec<ConflictFile>, GitServiceError> {
    let out = run_git(cwd, &["diff", "--name-only", "--diff-filter=U"])?;
    let text = String::from_utf8(out.stdout).map_err(|_| GitServiceError::Utf8Decode)?;
    Ok(text
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty())
        .map(|line| ConflictFile {
            path: line.to_string(),
        })
        .collect())
}

pub fn conflict_apply_choice(
    cwd: &Path,
    path: &str,
    choice: ConflictChoice,
) -> Result<(), GitServiceError> {
    let strategy = match choice {
        ConflictChoice::Ours => "--ours",
        ConflictChoice::Theirs => "--theirs",
    };
    let _ = run_git(cwd, &["checkout", strategy, "--", path])?;
    Ok(())
}

pub fn mark_conflict_resolved(cwd: &Path, paths: &[String]) -> Result<(), GitServiceError> {
    stage_paths(cwd, paths)
}

pub fn conflict_session_snapshot(
    cwd: &Path,
) -> Result<Option<ConflictSessionSnapshot>, GitServiceError> {
    let Some(state) = detect_conflict_state(cwd)? else {
        return Ok(None);
    };
    let files = list_conflicted_files(cwd)?;
    Ok(Some(ConflictSessionSnapshot { state, files }))
}

pub fn conflict_continue(cwd: &Path) -> Result<(), GitServiceError> {
    match detect_conflict_state(cwd)? {
        Some(ConflictState::Merge) => merge_continue(cwd),
        Some(ConflictState::Rebase) => rebase_continue(cwd),
        Some(ConflictState::CherryPick) => cherry_pick_continue(cwd),
        None => Err(GitServiceError::ParseError(
            "no conflict session in progress".to_string(),
        )),
    }
}

pub fn conflict_abort(cwd: &Path) -> Result<(), GitServiceError> {
    match detect_conflict_state(cwd)? {
        Some(ConflictState::Merge) => merge_abort(cwd),
        Some(ConflictState::Rebase) => rebase_abort(cwd),
        Some(ConflictState::CherryPick) => cherry_pick_abort(cwd),
        None => Err(GitServiceError::ParseError(
            "no conflict session in progress".to_string(),
        )),
    }
}

pub fn revert_commit(cwd: &Path, oid: &str) -> Result<(), GitServiceError> {
    if is_merge_commit(cwd, oid)? {
        return Err(GitServiceError::ParseError(
            "revert of merge commit is not supported without parent selection".to_string(),
        ));
    }
    let _ = run_git(cwd, &["revert", "--no-edit", oid])?;
    Ok(())
}

pub fn reset_ref(cwd: &Path, mode: ResetMode, target: &str) -> Result<(), GitServiceError> {
    let mode_flag = match mode {
        ResetMode::Soft => "--soft",
        ResetMode::Mixed => "--mixed",
        ResetMode::Hard => "--hard",
    };
    let _ = run_git(cwd, &["reset", mode_flag, target])?;
    Ok(())
}

pub fn create_rebase_plan(cwd: &Path, base_ref: &str) -> Result<RebasePlan, GitServiceError> {
    let base_oid = resolve_ref_oid(cwd, base_ref).ok();
    let head_name = repo_open(cwd)?.head;
    let commits = commit_log_range(cwd, base_ref, "HEAD", 200)?;
    let autosquash_aware = commits
        .iter()
        .any(|item| item.summary.starts_with("fixup!") || item.summary.starts_with("squash!"));
    let entries = commits
        .into_iter()
        .map(|item| RebasePlanEntry {
            oid: item.oid,
            warnings: if item.summary.starts_with("fixup!") || item.summary.starts_with("squash!") {
                vec!["autosquash marker detected".to_string()]
            } else {
                Vec::new()
            },
            summary: item.summary,
            action: RebaseAction::Pick,
        })
        .collect::<Vec<_>>();

    let published_history_warning = head_name.filter(|name| !name.is_empty()).and_then(|head| {
        let upstream = format!("{head}@{{upstream}}");
        if run_git(cwd, &["rev-parse", "--verify", &upstream]).is_ok() {
            Some("Published history rewrite warning: branch has upstream tracking".to_string())
        } else {
            None
        }
    });

    Ok(RebasePlan {
        base_ref: base_ref.to_string(),
        base_oid,
        entries,
        autosquash_aware,
        published_history_warning,
    })
}

pub fn execute_rebase_plan(
    cwd: &Path,
    plan: &RebasePlan,
    autosquash: bool,
) -> Result<(), GitServiceError> {
    let todo = render_rebase_todo(plan);
    let todo_file = unique_temp_file("branchforge-rebase-todo", "txt");
    let editor_file = unique_temp_file("branchforge-sequence-editor", "sh");

    std::fs::write(&todo_file, todo)
        .map_err(|err| GitServiceError::ProcessLaunch(err.to_string()))?;
    std::fs::write(
        &editor_file,
        "#!/usr/bin/env sh\ncat \"$BF_REBASE_TODO\" > \"$1\"\n",
    )
    .map_err(|err| GitServiceError::ProcessLaunch(err.to_string()))?;

    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let perms = std::fs::Permissions::from_mode(0o755);
        let _ = std::fs::set_permissions(&editor_file, perms);
    }

    let mut env = vec![
        (
            "GIT_SEQUENCE_EDITOR",
            editor_file.to_string_lossy().to_string(),
        ),
        ("BF_REBASE_TODO", todo_file.to_string_lossy().to_string()),
    ];
    if autosquash {
        env.push(("GIT_EDITOR", "true".to_string()));
    }

    let mut args = vec!["rebase", "-i"];
    if autosquash {
        args.push("--autosquash");
    }
    args.push(plan.base_ref.as_str());

    let result = run_git_with_env(cwd, &args, &env);
    let _ = std::fs::remove_file(todo_file);
    let _ = std::fs::remove_file(editor_file);
    result.map(|_| ())
}

pub fn rebase_continue(cwd: &Path) -> Result<(), GitServiceError> {
    continue_with_editor_fallback(cwd, &["rebase", "--continue"])
}

pub fn rebase_skip(cwd: &Path) -> Result<(), GitServiceError> {
    let _ = run_git(cwd, &["rebase", "--skip"])?;
    Ok(())
}

pub fn rebase_abort(cwd: &Path) -> Result<(), GitServiceError> {
    let _ = run_git(cwd, &["rebase", "--abort"])?;
    Ok(())
}

pub fn detect_rebase_session_hook(
    cwd: &Path,
) -> Result<Option<RebaseSessionHook>, GitServiceError> {
    let rebase_merge = git_path(cwd, "rebase-merge")?;
    let rebase_apply = git_path(cwd, "rebase-apply")?;
    if !rebase_merge.exists() && !rebase_apply.exists() {
        return Ok(None);
    }

    let base_dir = if rebase_merge.exists() {
        rebase_merge
    } else {
        rebase_apply
    };
    let current_step = read_usize_file(&base_dir.join("msgnum"))
        .or_else(|| read_usize_file(&base_dir.join("next")));
    let total_steps =
        read_usize_file(&base_dir.join("end")).or_else(|| read_usize_file(&base_dir.join("last")));
    Ok(Some(RebaseSessionHook {
        active: true,
        current_step,
        total_steps,
    }))
}

pub fn stage_hunk(cwd: &Path, path: &str, hunk_index: usize) -> Result<(), GitServiceError> {
    let hunks =
        diff_worktree_raw(cwd, &[path.to_string()]).map(|text| parse_unified_diff_hunks(&text))?;
    let hunk = hunks
        .iter()
        .find(|item| item.file_path == path && item.hunk_index == hunk_index)
        .ok_or_else(|| GitServiceError::ParseError("hunk not found".to_string()))?;
    apply_patch(cwd, &hunk.patch, true, false)
}

pub fn unstage_hunk(cwd: &Path, path: &str, hunk_index: usize) -> Result<(), GitServiceError> {
    let hunks =
        diff_index_raw(cwd, &[path.to_string()]).map(|text| parse_unified_diff_hunks(&text))?;
    let hunk = hunks
        .iter()
        .find(|item| item.file_path == path && item.hunk_index == hunk_index)
        .ok_or_else(|| GitServiceError::ParseError("hunk not found".to_string()))?;
    apply_patch(cwd, &hunk.patch, true, true)
}

pub fn discard_hunk(cwd: &Path, path: &str, hunk_index: usize) -> Result<(), GitServiceError> {
    let hunks =
        diff_worktree_raw(cwd, &[path.to_string()]).map(|text| parse_unified_diff_hunks(&text))?;
    let hunk = hunks
        .iter()
        .find(|item| item.file_path == path && item.hunk_index == hunk_index)
        .ok_or_else(|| GitServiceError::ParseError("hunk not found".to_string()))?;
    apply_patch(cwd, &hunk.patch, false, true)
}

pub fn stage_lines(
    cwd: &Path,
    path: &str,
    hunk_index: usize,
    line_indices: &[usize],
) -> Result<(), GitServiceError> {
    let hunk = diff_worktree_raw(cwd, &[path.to_string()])
        .map(|text| parse_unified_diff_hunks(&text))?
        .into_iter()
        .find(|item| item.file_path == path && item.hunk_index == hunk_index)
        .ok_or_else(|| GitServiceError::ParseError("hunk not found".to_string()))?;
    let index_text = load_index_file_text(cwd, path)?;
    let worktree_text = load_worktree_file_text(cwd, path)?;
    let target = build_partial_hunk_target(
        &index_text,
        &worktree_text,
        &hunk,
        line_indices,
        PartialHunkMode::ApplySelected,
    )?;
    if let Some(patch) = render_file_patch(path, &index_text, &target)? {
        apply_patch(cwd, &patch, true, false)?;
    }
    Ok(())
}

pub fn unstage_lines(
    cwd: &Path,
    path: &str,
    hunk_index: usize,
    line_indices: &[usize],
) -> Result<(), GitServiceError> {
    let hunk = diff_index_raw(cwd, &[path.to_string()])
        .map(|text| parse_unified_diff_hunks(&text))?
        .into_iter()
        .find(|item| item.file_path == path && item.hunk_index == hunk_index)
        .ok_or_else(|| GitServiceError::ParseError("hunk not found".to_string()))?;
    let head_text = load_head_file_text(cwd, path)?;
    let index_text = load_index_file_text(cwd, path)?;
    let target = build_partial_hunk_target(
        &head_text,
        &index_text,
        &hunk,
        line_indices,
        PartialHunkMode::ApplyUnselected,
    )?;
    if let Some(patch) = render_file_patch(path, &index_text, &target)? {
        apply_patch(cwd, &patch, true, false)?;
    }
    Ok(())
}

pub fn discard_lines(
    cwd: &Path,
    path: &str,
    hunk_index: usize,
    line_indices: &[usize],
) -> Result<(), GitServiceError> {
    let hunk = diff_worktree_raw(cwd, &[path.to_string()])
        .map(|text| parse_unified_diff_hunks(&text))?
        .into_iter()
        .find(|item| item.file_path == path && item.hunk_index == hunk_index)
        .ok_or_else(|| GitServiceError::ParseError("hunk not found".to_string()))?;
    let index_text = load_index_file_text(cwd, path)?;
    let worktree_text = load_worktree_file_text(cwd, path)?;
    let target = build_partial_hunk_target(
        &index_text,
        &worktree_text,
        &hunk,
        line_indices,
        PartialHunkMode::ApplyUnselected,
    )?;
    if let Some(patch) = render_file_patch(path, &worktree_text, &target)? {
        apply_patch(cwd, &patch, false, false)?;
    }
    Ok(())
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum PartialHunkMode {
    ApplySelected,
    ApplyUnselected,
}

fn build_partial_hunk_target(
    old_text: &str,
    new_text: &str,
    hunk: &DiffHunk,
    line_indices: &[usize],
    mode: PartialHunkMode,
) -> Result<String, GitServiceError> {
    let old_lines = split_lines_preserving_endings(old_text);
    let new_lines = split_lines_preserving_endings(new_text);
    let (old_start, old_count, new_start, new_count) = parse_hunk_header(&hunk.header)?;
    let old_start_idx = old_start.saturating_sub(1);
    let new_start_idx = new_start.saturating_sub(1);
    let old_end_idx = old_start_idx
        .checked_add(old_count)
        .ok_or_else(|| GitServiceError::ParseError("hunk old range overflow".to_string()))?;
    let new_end_idx = new_start_idx
        .checked_add(new_count)
        .ok_or_else(|| GitServiceError::ParseError("hunk new range overflow".to_string()))?;
    if old_end_idx > old_lines.len() || new_end_idx > new_lines.len() {
        return Err(GitServiceError::ParseError(
            "hunk range exceeds file content".to_string(),
        ));
    }

    let selected = line_indices.iter().copied().collect::<BTreeSet<_>>();
    let mut target = old_lines[..old_start_idx].to_vec();
    let mut old_cursor = old_start_idx;
    let mut new_cursor = new_start_idx;
    let mut change_count = 0usize;

    for line in hunk.lines.iter().skip(1) {
        if line == "\\ No newline at end of file" {
            continue;
        }
        let marker = line
            .chars()
            .next()
            .ok_or_else(|| GitServiceError::ParseError("empty hunk line".to_string()))?;
        match marker {
            ' ' => {
                let old_line = old_lines.get(old_cursor).ok_or_else(|| {
                    GitServiceError::ParseError("old hunk cursor out of range".to_string())
                })?;
                let new_line = new_lines.get(new_cursor).ok_or_else(|| {
                    GitServiceError::ParseError("new hunk cursor out of range".to_string())
                })?;
                if old_line != new_line {
                    return Err(GitServiceError::ParseError(
                        "context lines diverged while building partial hunk".to_string(),
                    ));
                }
                target.push(new_line.clone());
                old_cursor += 1;
                new_cursor += 1;
            }
            '-' => {
                let apply_change = should_apply_partial_change(&selected, change_count, mode);
                let old_line = old_lines.get(old_cursor).ok_or_else(|| {
                    GitServiceError::ParseError("old deletion cursor out of range".to_string())
                })?;
                if apply_change {
                    old_cursor += 1;
                } else {
                    target.push(old_line.clone());
                    old_cursor += 1;
                }
                change_count += 1;
            }
            '+' => {
                let apply_change = should_apply_partial_change(&selected, change_count, mode);
                let new_line = new_lines.get(new_cursor).ok_or_else(|| {
                    GitServiceError::ParseError("new addition cursor out of range".to_string())
                })?;
                if apply_change {
                    target.push(new_line.clone());
                }
                new_cursor += 1;
                change_count += 1;
            }
            other => {
                return Err(GitServiceError::ParseError(format!(
                    "unsupported hunk line marker: {other}"
                )));
            }
        }
    }

    if old_cursor != old_end_idx || new_cursor != new_end_idx {
        return Err(GitServiceError::ParseError(
            "partial hunk cursors did not consume expected range".to_string(),
        ));
    }
    if change_count == 0 {
        return Err(GitServiceError::ParseError(
            "hunk does not contain changed lines".to_string(),
        ));
    }
    if selected.is_empty() {
        return Err(GitServiceError::ParseError(
            "line selection cannot be empty".to_string(),
        ));
    }
    if let Some(out_of_range) = selected.iter().find(|index| **index >= change_count) {
        return Err(GitServiceError::ParseError(format!(
            "selected line index out of range: {out_of_range}"
        )));
    }

    target.extend_from_slice(&old_lines[old_end_idx..]);
    Ok(target.concat())
}

fn should_apply_partial_change(
    selected: &BTreeSet<usize>,
    change_index: usize,
    mode: PartialHunkMode,
) -> bool {
    let explicitly_selected = selected.contains(&change_index);
    match mode {
        PartialHunkMode::ApplySelected => explicitly_selected,
        PartialHunkMode::ApplyUnselected => !explicitly_selected,
    }
}

fn split_lines_preserving_endings(text: &str) -> Vec<String> {
    if text.is_empty() {
        Vec::new()
    } else {
        text.split_inclusive('\n').map(str::to_string).collect()
    }
}

fn parse_hunk_header(header: &str) -> Result<(usize, usize, usize, usize), GitServiceError> {
    let parts = header.split_whitespace().collect::<Vec<_>>();
    if parts.len() < 3 || parts[0] != "@@" {
        return Err(GitServiceError::ParseError(format!(
            "invalid hunk header: {header}"
        )));
    }
    let (old_start, old_count) = parse_hunk_range(parts[1], '-')?;
    let (new_start, new_count) = parse_hunk_range(parts[2], '+')?;
    Ok((old_start, old_count, new_start, new_count))
}

fn parse_hunk_range(raw: &str, prefix: char) -> Result<(usize, usize), GitServiceError> {
    let body = raw
        .strip_prefix(prefix)
        .ok_or_else(|| GitServiceError::ParseError(format!("invalid hunk range: {raw}")))?;
    let (start, count) = match body.split_once(',') {
        Some((start, count)) => (start, count),
        None => (body, "1"),
    };
    let start = start
        .parse::<usize>()
        .map_err(|_| GitServiceError::ParseError(format!("invalid hunk start: {raw}")))?;
    let count = count
        .parse::<usize>()
        .map_err(|_| GitServiceError::ParseError(format!("invalid hunk count: {raw}")))?;
    Ok((start, count))
}

fn load_worktree_file_text(cwd: &Path, path: &str) -> Result<String, GitServiceError> {
    let full_path = cwd.join(path);
    match std::fs::read(&full_path) {
        Ok(bytes) => String::from_utf8(bytes).map_err(|_| GitServiceError::Utf8Decode),
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => Ok(String::new()),
        Err(err) => Err(GitServiceError::ProcessLaunch(err.to_string())),
    }
}

fn load_index_file_text(cwd: &Path, path: &str) -> Result<String, GitServiceError> {
    load_git_object_text(cwd, &format!(":{path}"))
}

fn load_head_file_text(cwd: &Path, path: &str) -> Result<String, GitServiceError> {
    load_git_object_text(cwd, &format!("HEAD:{path}"))
}

fn load_git_object_text(cwd: &Path, spec: &str) -> Result<String, GitServiceError> {
    let exists = Command::new("git")
        .args(["cat-file", "-e", spec])
        .current_dir(cwd)
        .output()
        .map_err(|err| GitServiceError::ProcessLaunch(err.to_string()))?;
    let exit_code = exists.status.code().unwrap_or(-1);
    if exit_code != 0 {
        if exit_code == 128 {
            return Ok(String::new());
        }
        let stderr =
            String::from_utf8(exists.stderr).unwrap_or_else(|_| "<non-utf8 stderr>".to_string());
        return Err(GitServiceError::GitError { exit_code, stderr });
    }
    let out = run_git(cwd, &["show", spec])?;
    String::from_utf8(out.stdout).map_err(|_| GitServiceError::Utf8Decode)
}

fn render_file_patch(
    path: &str,
    before: &str,
    after: &str,
) -> Result<Option<String>, GitServiceError> {
    if before == after {
        return Ok(None);
    }

    let before_path = unique_temp_file("branchforge-before", "txt");
    let after_path = unique_temp_file("branchforge-after", "txt");
    std::fs::write(&before_path, before)
        .map_err(|err| GitServiceError::ProcessLaunch(err.to_string()))?;
    std::fs::write(&after_path, after)
        .map_err(|err| GitServiceError::ProcessLaunch(err.to_string()))?;

    let output = Command::new("git")
        .args([
            "diff",
            "--no-index",
            "--text",
            "--unified=3",
            "--",
            before_path.to_string_lossy().as_ref(),
            after_path.to_string_lossy().as_ref(),
        ])
        .output()
        .map_err(|err| GitServiceError::ProcessLaunch(err.to_string()))?;

    let _ = std::fs::remove_file(&before_path);
    let _ = std::fs::remove_file(&after_path);

    let exit_code = output.status.code().unwrap_or(-1);
    if exit_code != 0 && exit_code != 1 {
        let stderr =
            String::from_utf8(output.stderr).unwrap_or_else(|_| "<non-utf8 stderr>".to_string());
        return Err(GitServiceError::GitError { exit_code, stderr });
    }

    let before_ref = format!("a{}", before_path.to_string_lossy());
    let after_ref = format!("b{}", after_path.to_string_lossy());
    let mut patch = String::from_utf8(output.stdout).map_err(|_| GitServiceError::Utf8Decode)?;
    if patch.trim().is_empty() {
        return Ok(None);
    }
    patch = patch.replace(&before_ref, &format!("a/{path}"));
    patch = patch.replace(&after_ref, &format!("b/{path}"));
    Ok(Some(patch))
}

fn apply_patch(
    cwd: &Path,
    patch: &str,
    cached: bool,
    reverse: bool,
) -> Result<(), GitServiceError> {
    let mut args = vec!["apply".to_string()];
    if cached {
        args.push("--cached".to_string());
    }
    if reverse {
        args.push("-R".to_string());
    }
    let refs: Vec<&str> = args.iter().map(String::as_str).collect();
    let _ = run_git_with_input(cwd, &refs, patch.as_bytes())?;
    Ok(())
}

fn run_git_with_env(
    cwd: &Path,
    args: &[&str],
    env: &[(&str, String)],
) -> Result<GitRunResult, GitServiceError> {
    let mut command = Command::new("git");
    command.args(args).current_dir(cwd);
    for (key, value) in env {
        command.env(key, value);
    }
    let output = command
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

fn resolve_ref_oid(cwd: &Path, reference: &str) -> Result<String, GitServiceError> {
    let out = run_git(cwd, &["rev-parse", "--verify", reference])?;
    String::from_utf8(out.stdout)
        .map(|text| text.trim().to_string())
        .map_err(|_| GitServiceError::Utf8Decode)
}

fn continue_with_editor_fallback(cwd: &Path, args: &[&str]) -> Result<(), GitServiceError> {
    if run_git(cwd, args).is_ok() {
        return Ok(());
    }
    // Keep continue flow non-interactive for automated host/tests.
    let _ = run_git_with_env(cwd, args, &[("GIT_EDITOR", "true".to_string())])?;
    Ok(())
}

fn render_rebase_todo(plan: &RebasePlan) -> String {
    let mut lines = Vec::new();
    for entry in &plan.entries {
        let action = match entry.action {
            RebaseAction::Pick => "pick",
            RebaseAction::Reword => "reword",
            RebaseAction::Edit => "edit",
            RebaseAction::Squash => "squash",
            RebaseAction::Fixup => "fixup",
            RebaseAction::Drop => "drop",
        };
        lines.push(format!("{action} {} {}", entry.oid, entry.summary));
    }
    if lines.is_empty() {
        "\n".to_string()
    } else {
        format!("{}\n", lines.join("\n"))
    }
}

fn unique_temp_file(prefix: &str, ext: &str) -> std::path::PathBuf {
    static COUNTER: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0);
    let seq = COUNTER.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    std::env::temp_dir().join(format!("{prefix}-{nanos}-{seq}.{ext}"))
}

fn read_usize_file(path: &Path) -> Option<usize> {
    let text = std::fs::read_to_string(path).ok()?;
    text.trim().parse::<usize>().ok()
}

fn diff_worktree_raw(cwd: &Path, paths: &[String]) -> Result<String, GitServiceError> {
    let mut args = vec!["diff".to_string(), "--patch".to_string()];
    if !paths.is_empty() {
        args.push("--".to_string());
        args.extend(paths.iter().cloned());
    }
    let refs: Vec<&str> = args.iter().map(String::as_str).collect();
    let out = run_git(cwd, &refs)?;
    String::from_utf8(out.stdout).map_err(|_| GitServiceError::Utf8Decode)
}

fn diff_index_raw(cwd: &Path, paths: &[String]) -> Result<String, GitServiceError> {
    let mut args = vec![
        "diff".to_string(),
        "--cached".to_string(),
        "--patch".to_string(),
    ];
    if !paths.is_empty() {
        args.push("--".to_string());
        args.extend(paths.iter().cloned());
    }
    let refs: Vec<&str> = args.iter().map(String::as_str).collect();
    let out = run_git(cwd, &refs)?;
    String::from_utf8(out.stdout).map_err(|_| GitServiceError::Utf8Decode)
}

fn diff_between_refs_raw(
    cwd: &Path,
    base_ref: &str,
    head_ref: &str,
) -> Result<String, GitServiceError> {
    let out = run_git(
        cwd,
        &["diff", "--patch", &format!("{base_ref}..{head_ref}")],
    )?;
    String::from_utf8(out.stdout).map_err(|_| GitServiceError::Utf8Decode)
}

fn truncate_diff(mut text: String, max_bytes: usize) -> String {
    if text.len() > max_bytes {
        text.truncate(max_bytes);
        text.push_str("\n... diff truncated ...\n");
    }
    text
}

fn parse_unified_diff_hunks(text: &str) -> Vec<DiffHunk> {
    let mut hunks = Vec::new();
    let mut current_file: Option<String> = None;
    let mut header_lines: Vec<String> = Vec::new();
    let mut hunk_lines: Vec<String> = Vec::new();
    let mut hunk_header = String::new();
    let mut in_hunk = false;
    let mut hunk_index = 0;

    for line in text.lines() {
        if line.starts_with("diff --git ") {
            if in_hunk {
                push_hunk(
                    &mut hunks,
                    &current_file,
                    &header_lines,
                    &hunk_lines,
                    &hunk_header,
                    &mut hunk_index,
                );
            }
            current_file = line
                .split_whitespace()
                .nth(3)
                .map(|value| value.trim_start_matches("b/").to_string());
            header_lines = vec![line.to_string()];
            hunk_lines.clear();
            hunk_header.clear();
            in_hunk = false;
            hunk_index = 0;
            continue;
        }

        if current_file.is_none() {
            continue;
        }

        if line.starts_with("@@") {
            if in_hunk {
                push_hunk(
                    &mut hunks,
                    &current_file,
                    &header_lines,
                    &hunk_lines,
                    &hunk_header,
                    &mut hunk_index,
                );
                hunk_lines.clear();
            }
            in_hunk = true;
            hunk_header = line.to_string();
            hunk_lines.push(line.to_string());
            continue;
        }

        if in_hunk {
            hunk_lines.push(line.to_string());
            continue;
        }

        if line.starts_with("index ")
            || line.starts_with("--- ")
            || line.starts_with("+++ ")
            || line.starts_with("new file")
            || line.starts_with("deleted file")
            || line.starts_with("rename ")
            || line.starts_with("similarity ")
        {
            header_lines.push(line.to_string());
        }
    }

    if in_hunk {
        push_hunk(
            &mut hunks,
            &current_file,
            &header_lines,
            &hunk_lines,
            &hunk_header,
            &mut hunk_index,
        );
    }

    hunks
}

fn push_hunk(
    hunks: &mut Vec<DiffHunk>,
    current_file: &Option<String>,
    header_lines: &[String],
    hunk_lines: &[String],
    hunk_header: &str,
    hunk_index: &mut usize,
) {
    if let Some(file_path) = current_file
        && !header_lines.is_empty()
        && !hunk_lines.is_empty()
    {
        let mut patch_lines = Vec::new();
        patch_lines.extend(header_lines.iter().cloned());
        patch_lines.extend(hunk_lines.iter().cloned());
        let mut patch = patch_lines.join("\n");
        patch.push('\n');
        hunks.push(DiffHunk {
            file_path: file_path.clone(),
            hunk_index: *hunk_index,
            header: hunk_header.to_string(),
            lines: hunk_lines.to_vec(),
            patch,
        });
        *hunk_index += 1;
    }
}

pub fn list_local_branches(cwd: &Path) -> Result<Vec<BranchInfo>, GitServiceError> {
    let out = run_git(
        cwd,
        &[
            "for-each-ref",
            "--format=%(refname:short)%00%(HEAD)%00%(upstream:short)",
            "refs/heads",
        ],
    )?;
    let text = String::from_utf8(out.stdout).map_err(|_| GitServiceError::Utf8Decode)?;
    let mut branches = Vec::new();

    for line in text.lines() {
        if line.trim().is_empty() {
            continue;
        }
        let parts: Vec<&str> = line.split('\0').collect();
        if parts.len() < 2 {
            return Err(GitServiceError::ParseError(
                "invalid branch record".to_string(),
            ));
        }
        let name = parts[0].to_string();
        let is_current = parts[1] == "*";
        let upstream = parts.get(2).and_then(|value| {
            if value.trim().is_empty() {
                None
            } else {
                Some(value.to_string())
            }
        });
        branches.push(BranchInfo {
            name,
            is_current,
            upstream,
        });
    }

    Ok(branches)
}

pub fn worktree_is_clean(cwd: &Path) -> Result<bool, GitServiceError> {
    let out = run_git(cwd, &["status", "--porcelain"])?;
    Ok(out.stdout.is_empty())
}

pub fn checkout_branch(cwd: &Path, name: &str) -> Result<(), GitServiceError> {
    let _ = run_git(cwd, &["checkout", name])?;
    Ok(())
}

pub fn create_branch(cwd: &Path, name: &str) -> Result<(), GitServiceError> {
    let _ = run_git(cwd, &["branch", name])?;
    Ok(())
}

pub fn rename_branch(cwd: &Path, old: &str, new: &str) -> Result<(), GitServiceError> {
    let _ = run_git(cwd, &["branch", "-m", old, new])?;
    Ok(())
}

pub fn delete_branch(cwd: &Path, name: &str) -> Result<(), GitServiceError> {
    let _ = run_git(cwd, &["branch", "-d", name])?;
    Ok(())
}

pub fn list_tags(cwd: &Path) -> Result<Vec<String>, GitServiceError> {
    let out = run_git(cwd, &["tag", "--list"])?;
    let text = String::from_utf8(out.stdout).map_err(|_| GitServiceError::Utf8Decode)?;
    let tags = text
        .lines()
        .filter(|line| !line.trim().is_empty())
        .map(|line| line.to_string())
        .collect();
    Ok(tags)
}

pub fn create_tag(cwd: &Path, name: &str) -> Result<(), GitServiceError> {
    let _ = run_git(cwd, &["tag", name])?;
    Ok(())
}

pub fn delete_tag(cwd: &Path, name: &str) -> Result<(), GitServiceError> {
    let _ = run_git(cwd, &["tag", "-d", name])?;
    Ok(())
}

pub fn inspect_tag_target(cwd: &Path, name: &str) -> Result<TagTarget, GitServiceError> {
    let rev = run_git(cwd, &["rev-list", "-n", "1", name])?;
    let oid = String::from_utf8(rev.stdout)
        .map_err(|_| GitServiceError::Utf8Decode)?
        .trim()
        .to_string();
    if oid.is_empty() {
        return Err(GitServiceError::ParseError(
            "tag target oid is empty".to_string(),
        ));
    }

    let kind = run_git(cwd, &["cat-file", "-t", &oid])?;
    let object_type = String::from_utf8(kind.stdout)
        .map_err(|_| GitServiceError::Utf8Decode)?
        .trim()
        .to_string();

    Ok(TagTarget {
        name: name.to_string(),
        oid,
        object_type,
    })
}

pub fn checkout_tag(cwd: &Path, name: &str) -> Result<(), GitServiceError> {
    let _ = run_git(cwd, &["checkout", name])?;
    Ok(())
}

pub fn discard_paths(cwd: &Path, paths: &[String]) -> Result<(), GitServiceError> {
    if paths.is_empty() {
        return Ok(());
    }
    let mut args = vec![
        "restore".to_string(),
        "--worktree".to_string(),
        "--".to_string(),
    ];
    args.extend(paths.iter().cloned());
    let refs: Vec<&str> = args.iter().map(String::as_str).collect();
    let _ = run_git(cwd, &refs)?;
    Ok(())
}

fn count_revisions(cwd: &Path, range: &str) -> Result<usize, GitServiceError> {
    let out = run_git(cwd, &["rev-list", "--count", range])?;
    let text = String::from_utf8(out.stdout).map_err(|_| GitServiceError::Utf8Decode)?;
    text.trim()
        .parse::<usize>()
        .map_err(|_| GitServiceError::ParseError("invalid revision count".to_string()))
}

fn commit_log_range(
    cwd: &Path,
    base_ref: &str,
    head_ref: &str,
    max_commits: usize,
) -> Result<Vec<CommitSummary>, GitServiceError> {
    let format = "--format=%H%x1f%an%x1f%ad%x1f%s";
    let max_count = max_commits.to_string();
    let range = format!("{base_ref}..{head_ref}");
    let out = run_git(
        cwd,
        &[
            "log",
            "--date=iso-strict",
            format,
            "--max-count",
            &max_count,
            &range,
        ],
    )?;
    let text = String::from_utf8(out.stdout).map_err(|_| GitServiceError::Utf8Decode)?;
    let mut commits = Vec::new();
    for line in text.lines() {
        if line.trim().is_empty() {
            continue;
        }
        let parts: Vec<&str> = line.split('\x1f').collect();
        if parts.len() != 4 {
            return Err(GitServiceError::ParseError(
                "invalid compare commit line".to_string(),
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

fn is_merge_commit(cwd: &Path, oid: &str) -> Result<bool, GitServiceError> {
    let out = run_git(cwd, &["rev-list", "--parents", "-n", "1", oid])?;
    let text = String::from_utf8(out.stdout).map_err(|_| GitServiceError::Utf8Decode)?;
    let parent_count = text.split_whitespace().skip(1).count();
    Ok(parent_count > 1)
}

pub fn commit_amend(cwd: &Path, message: Option<&str>) -> Result<(), GitServiceError> {
    match message {
        Some(msg) => {
            let trimmed = msg.trim();
            if trimmed.is_empty() {
                return Err(GitServiceError::ParseError(
                    "commit message cannot be empty".to_string(),
                ));
            }
            let _ = run_git(cwd, &["commit", "--amend", "-m", trimmed])?;
        }
        None => {
            let _ = run_git(cwd, &["commit", "--amend", "--no-edit"])?;
        }
    }
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
            "u" => {
                let _xy = parts
                    .next()
                    .ok_or_else(|| GitServiceError::ParseError("missing XY status".to_string()))?;
                let path = parts
                    .last()
                    .ok_or_else(|| {
                        GitServiceError::ParseError("missing path in unmerged entry".to_string())
                    })?
                    .to_string();
                summary.unstaged.push(path);
                idx += 1;
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

    fn git_lfs_available() -> bool {
        std::process::Command::new("git")
            .args(["lfs", "version"])
            .output()
            .map(|output| output.status.success())
            .unwrap_or(false)
    }

    fn init_lfs_clone_pair(label: &str) -> Option<(std::path::PathBuf, String)> {
        if !git_lfs_available() {
            return None;
        }

        let root = unique_temp_dir();
        let origin = root.join(format!("{label}-origin.git"));
        let source = root.join(format!("{label}-source"));
        let clone = root.join(format!("{label}-clone"));
        let file_name = "payload.bin";
        let payload = "branchforge-lfs-payload\n".repeat(64);

        assert!(std::fs::create_dir_all(&source).is_ok());
        assert!(run_git(&source, &["init"]).is_ok());
        assert!(run_git(&source, &["config", "user.email", "dev@example.com"]).is_ok());
        assert!(run_git(&source, &["config", "user.name", "Dev User"]).is_ok());
        assert!(run_git(&source, &["lfs", "install", "--local"]).is_ok());
        assert!(run_git(&source, &["lfs", "track", "*.bin"]).is_ok());
        assert!(std::fs::write(source.join(file_name), &payload).is_ok());
        assert!(
            stage_paths(
                &source,
                &[".gitattributes".to_string(), file_name.to_string()]
            )
            .is_ok()
        );
        assert!(commit_create(&source, "add lfs payload").is_ok());

        assert!(std::fs::create_dir_all(&origin).is_ok());
        assert!(
            std::process::Command::new("git")
                .args(["init", "--bare", origin.to_string_lossy().as_ref()])
                .output()
                .map(|output| output.status.success())
                .unwrap_or(false)
        );
        assert!(
            run_git(
                &source,
                &["remote", "add", "origin", origin.to_string_lossy().as_ref()]
            )
            .is_ok()
        );
        assert!(run_git(&source, &["push", "-u", "origin", "HEAD"]).is_ok());

        let branch = run_git(&source, &["branch", "--show-current"])
            .ok()
            .and_then(|out| String::from_utf8(out.stdout).ok())
            .map(|text| text.trim().to_string())
            .filter(|value| !value.is_empty())
            .unwrap_or_else(|| "master".to_string());
        assert!(
            std::process::Command::new("git")
                .args([
                    "--git-dir",
                    origin.to_string_lossy().as_ref(),
                    "symbolic-ref",
                    "HEAD",
                    &format!("refs/heads/{branch}"),
                ])
                .output()
                .map(|output| output.status.success())
                .unwrap_or(false)
        );

        assert!(
            std::process::Command::new("git")
                .env("GIT_LFS_SKIP_SMUDGE", "1")
                .args([
                    "clone",
                    origin.to_string_lossy().as_ref(),
                    clone.to_string_lossy().as_ref(),
                ])
                .output()
                .map(|output| output.status.success())
                .unwrap_or(false)
        );
        assert!(run_git(&clone, &["lfs", "install", "--local"]).is_ok());

        Some((clone, payload))
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
    fn parses_unmerged_status_kind() {
        let fixture = b"u UU N... 100644 100644 100644 abcdef abcdef conflict.txt\0";
        let parsed = parse_status_porcelain_v2_z(fixture);
        assert!(parsed.is_ok());
        if let Ok(summary) = parsed {
            assert!(summary.unstaged.iter().any(|path| path == "conflict.txt"));
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

    #[test]
    fn branch_list_create_checkout_rename_delete() {
        let repo_dir = unique_temp_dir();
        assert!(std::fs::create_dir_all(&repo_dir).is_ok());
        assert!(run_git(&repo_dir, &["init"]).is_ok());
        assert!(run_git(&repo_dir, &["config", "user.email", "dev@example.com"]).is_ok());
        assert!(run_git(&repo_dir, &["config", "user.name", "Dev User"]).is_ok());

        assert!(std::fs::write(repo_dir.join("README.md"), "hello\n").is_ok());
        assert!(stage_paths(&repo_dir, &["README.md".to_string()]).is_ok());
        assert!(commit_create(&repo_dir, "base").is_ok());

        assert!(create_branch(&repo_dir, "feature").is_ok());
        let branches = list_local_branches(&repo_dir).expect("branches");
        assert!(branches.iter().any(|b| b.name == "feature"));

        assert!(checkout_branch(&repo_dir, "feature").is_ok());
        let branches = list_local_branches(&repo_dir).expect("branches");
        assert!(branches.iter().any(|b| b.name == "feature" && b.is_current));

        assert!(rename_branch(&repo_dir, "feature", "feature-renamed").is_ok());
        let branches = list_local_branches(&repo_dir).expect("branches");
        assert!(branches.iter().any(|b| b.name == "feature-renamed"));

        assert!(
            checkout_branch(&repo_dir, "main").is_ok()
                || checkout_branch(&repo_dir, "master").is_ok()
        );
        assert!(delete_branch(&repo_dir, "feature-renamed").is_ok());

        let _ = std::fs::remove_dir_all(&repo_dir);
    }

    #[test]
    fn tag_list_create_and_checkout() {
        let repo_dir = unique_temp_dir();
        assert!(std::fs::create_dir_all(&repo_dir).is_ok());
        assert!(run_git(&repo_dir, &["init"]).is_ok());
        assert!(run_git(&repo_dir, &["config", "user.email", "dev@example.com"]).is_ok());
        assert!(run_git(&repo_dir, &["config", "user.name", "Dev User"]).is_ok());

        assert!(std::fs::write(repo_dir.join("README.md"), "hello\n").is_ok());
        assert!(stage_paths(&repo_dir, &["README.md".to_string()]).is_ok());
        assert!(commit_create(&repo_dir, "base").is_ok());

        assert!(create_tag(&repo_dir, "v0.1.0").is_ok());
        let tags = list_tags(&repo_dir).expect("tags");
        assert!(tags.iter().any(|tag| tag == "v0.1.0"));

        assert!(checkout_tag(&repo_dir, "v0.1.0").is_ok());

        let _ = std::fs::remove_dir_all(&repo_dir);
    }

    #[test]
    fn stage_and_unstage_hunk_roundtrip() {
        let repo_dir = unique_temp_dir();
        assert!(std::fs::create_dir_all(&repo_dir).is_ok());
        assert!(run_git(&repo_dir, &["init"]).is_ok());
        assert!(run_git(&repo_dir, &["config", "user.email", "dev@example.com"]).is_ok());
        assert!(run_git(&repo_dir, &["config", "user.name", "Dev User"]).is_ok());

        let file = repo_dir.join("hunk.txt");
        let mut lines = Vec::new();
        for idx in 1..=20 {
            lines.push(format!("line{idx}\n"));
        }
        let base = lines.concat();
        assert!(std::fs::write(&file, base).is_ok());
        assert!(stage_paths(&repo_dir, &["hunk.txt".to_string()]).is_ok());
        assert!(commit_create(&repo_dir, "base").is_ok());

        lines[0] = "line1-updated\n".to_string();
        lines[19] = "line20-updated\n".to_string();
        let updated = lines.concat();
        assert!(std::fs::write(&file, updated).is_ok());

        let stage_result = stage_hunk(&repo_dir, "hunk.txt", 0);
        assert!(stage_result.is_ok());

        let staged = diff_index_with_hunks(&repo_dir, &["hunk.txt".to_string()], 10_000);
        assert!(staged.is_ok());
        if let Ok(output) = staged {
            assert!(output.text.contains("line1-updated"));
            assert!(!output.text.contains("line20-updated"));
        }

        let unstage_result = unstage_hunk(&repo_dir, "hunk.txt", 0);
        assert!(unstage_result.is_ok());

        let staged_after = diff_index_with_hunks(&repo_dir, &["hunk.txt".to_string()], 10_000);
        assert!(staged_after.is_ok());
        if let Ok(output) = staged_after {
            assert!(output.text.trim().is_empty());
        }

        let _ = std::fs::remove_dir_all(&repo_dir);
    }

    #[test]
    fn discard_hunk_reverts_only_selected_hunk() {
        let repo_dir = unique_temp_dir();
        assert!(std::fs::create_dir_all(&repo_dir).is_ok());
        assert!(run_git(&repo_dir, &["init"]).is_ok());
        assert!(run_git(&repo_dir, &["config", "user.email", "dev@example.com"]).is_ok());
        assert!(run_git(&repo_dir, &["config", "user.name", "Dev User"]).is_ok());

        let file = repo_dir.join("discard_hunk.txt");
        let mut lines = Vec::new();
        for idx in 1..=20 {
            lines.push(format!("line{idx}\n"));
        }
        assert!(std::fs::write(&file, lines.concat()).is_ok());
        assert!(stage_paths(&repo_dir, &["discard_hunk.txt".to_string()]).is_ok());
        assert!(commit_create(&repo_dir, "base").is_ok());

        lines[0] = "line1-updated\n".to_string();
        lines[19] = "line20-updated\n".to_string();
        assert!(std::fs::write(&file, lines.concat()).is_ok());

        assert!(discard_hunk(&repo_dir, "discard_hunk.txt", 0).is_ok());
        let content = std::fs::read_to_string(&file).unwrap_or_default();
        assert!(content.contains("line1\n"));
        assert!(content.contains("line20-updated\n"));

        let _ = std::fs::remove_dir_all(&repo_dir);
    }

    #[test]
    fn stage_unstage_and_discard_selected_lines() {
        let repo_dir = unique_temp_dir();
        assert!(std::fs::create_dir_all(&repo_dir).is_ok());
        assert!(run_git(&repo_dir, &["init"]).is_ok());
        assert!(run_git(&repo_dir, &["config", "user.email", "dev@example.com"]).is_ok());
        assert!(run_git(&repo_dir, &["config", "user.name", "Dev User"]).is_ok());

        let file = repo_dir.join("lines.txt");
        assert!(std::fs::write(&file, "line1\nline2\nline3\nline4\n").is_ok());
        assert!(stage_paths(&repo_dir, &["lines.txt".to_string()]).is_ok());
        assert!(commit_create(&repo_dir, "base").is_ok());

        assert!(std::fs::write(&file, "line1-updated\nline2\nline3\nline3-added\nline4\n").is_ok());

        assert!(stage_lines(&repo_dir, "lines.txt", 0, &[0, 1]).is_ok());
        let staged = diff_index_with_hunks(&repo_dir, &["lines.txt".to_string()], 10_000)
            .expect("staged diff");
        assert!(staged.text.contains("line1-updated"));
        assert!(!staged.text.contains("line3-added"));

        assert!(stage_paths(&repo_dir, &["lines.txt".to_string()]).is_ok());
        let staged_all = diff_index_with_hunks(&repo_dir, &["lines.txt".to_string()], 10_000)
            .expect("staged all diff");
        assert!(staged_all.text.contains("line3-added"));

        assert!(unstage_lines(&repo_dir, "lines.txt", 0, &[2]).is_ok());
        let staged_after_unstage =
            diff_index_with_hunks(&repo_dir, &["lines.txt".to_string()], 10_000)
                .expect("staged after unstage");
        assert!(staged_after_unstage.text.contains("line1-updated"));
        assert!(!staged_after_unstage.text.contains("line3-added"));

        assert!(std::fs::write(&file, "line1-updated\nline2\nline3\nline3-added\nline4\n").is_ok());
        assert!(discard_lines(&repo_dir, "lines.txt", 0, &[0]).is_ok());
        let content = std::fs::read_to_string(&file).unwrap_or_default();
        assert!(content.contains("line1-updated\n"));
        assert!(!content.contains("line3-added\n"));

        let _ = std::fs::remove_dir_all(&repo_dir);
    }

    #[test]
    fn lfs_status_fetch_and_pull_work_when_runtime_is_available() {
        let Some((clone_dir, payload)) = init_lfs_clone_pair("lfs-runtime") else {
            return;
        };

        let pointer_before =
            std::fs::read_to_string(clone_dir.join("payload.bin")).unwrap_or_default();
        assert!(pointer_before.contains("git-lfs.github.com/spec/v1"));

        assert!(lfs_status(&clone_dir).is_ok());
        assert!(lfs_fetch(&clone_dir).is_ok());

        let pointer_after_fetch =
            std::fs::read_to_string(clone_dir.join("payload.bin")).unwrap_or_default();
        assert!(pointer_after_fetch.contains("git-lfs.github.com/spec/v1"));

        assert!(lfs_pull(&clone_dir).is_ok());
        let content_after_pull =
            std::fs::read_to_string(clone_dir.join("payload.bin")).unwrap_or_default();
        assert_eq!(content_after_pull, payload);

        let _ = std::fs::remove_dir_all(
            clone_dir
                .parent()
                .map(std::path::Path::to_path_buf)
                .unwrap_or(clone_dir),
        );
    }

    #[test]
    fn lfs_ops_fallback_cleanly_when_git_lfs_is_missing() {
        if git_lfs_available() {
            return;
        }

        let repo_dir = unique_temp_dir();
        assert!(std::fs::create_dir_all(&repo_dir).is_ok());
        assert!(run_git(&repo_dir, &["init"]).is_ok());

        let status = lfs_status(&repo_dir).unwrap_or_default();
        let fetch = lfs_fetch(&repo_dir).unwrap_or_default();
        let pull = lfs_pull(&repo_dir).unwrap_or_default();

        assert!(status.contains("git-lfs is not installed"));
        assert!(fetch.contains("git-lfs is not installed"));
        assert!(pull.contains("git-lfs is not installed"));

        let _ = std::fs::remove_dir_all(&repo_dir);
    }

    #[test]
    fn merge_cherry_pick_revert_and_reset_baseline() {
        let repo_dir = unique_temp_dir();
        assert!(std::fs::create_dir_all(&repo_dir).is_ok());
        assert!(run_git(&repo_dir, &["init"]).is_ok());
        assert!(run_git(&repo_dir, &["config", "user.email", "dev@example.com"]).is_ok());
        assert!(run_git(&repo_dir, &["config", "user.name", "Dev User"]).is_ok());

        let file = repo_dir.join("advanced.txt");
        assert!(std::fs::write(&file, "base\n").is_ok());
        assert!(stage_paths(&repo_dir, &["advanced.txt".to_string()]).is_ok());
        assert!(commit_create(&repo_dir, "base").is_ok());

        assert!(create_branch(&repo_dir, "feature").is_ok());
        assert!(checkout_branch(&repo_dir, "feature").is_ok());
        assert!(std::fs::write(&file, "base\nfeature\n").is_ok());
        assert!(stage_paths(&repo_dir, &["advanced.txt".to_string()]).is_ok());
        assert!(commit_create(&repo_dir, "feature commit").is_ok());

        assert!(
            checkout_branch(&repo_dir, "main").is_ok()
                || checkout_branch(&repo_dir, "master").is_ok()
        );
        assert!(merge_ref(&repo_dir, "feature", MergeMode::FastForward).is_ok());
        let merged = std::fs::read_to_string(&file).unwrap_or_default();
        assert!(merged.contains("feature"));

        assert!(std::fs::write(&file, "base\nfeature\nlocal\n").is_ok());
        assert!(stage_paths(&repo_dir, &["advanced.txt".to_string()]).is_ok());
        assert!(commit_create(&repo_dir, "local commit").is_ok());
        let head = commit_log_page(&repo_dir, 0, 1)
            .ok()
            .and_then(|mut page| page.pop())
            .map(|item| item.oid)
            .unwrap_or_default();
        assert!(!head.is_empty());

        assert!(revert_commit(&repo_dir, &head).is_ok());
        let reverted = std::fs::read_to_string(&file).unwrap_or_default();
        assert!(!reverted.contains("local"));

        assert!(std::fs::write(&file, "dirty\n").is_ok());
        assert!(reset_ref(&repo_dir, ResetMode::Hard, "HEAD").is_ok());
        let reset = std::fs::read_to_string(&file).unwrap_or_default();
        assert!(!reset.contains("dirty"));

        assert!(create_branch(&repo_dir, "pick-source").is_ok());
        assert!(checkout_branch(&repo_dir, "pick-source").is_ok());
        assert!(std::fs::write(repo_dir.join("pick.txt"), "picked\n").is_ok());
        assert!(stage_paths(&repo_dir, &["pick.txt".to_string()]).is_ok());
        assert!(commit_create(&repo_dir, "pick commit").is_ok());
        let pick_oid = commit_log_page(&repo_dir, 0, 1)
            .ok()
            .and_then(|mut page| page.pop())
            .map(|item| item.oid)
            .unwrap_or_default();
        assert!(!pick_oid.is_empty());

        assert!(
            checkout_branch(&repo_dir, "main").is_ok()
                || checkout_branch(&repo_dir, "master").is_ok()
        );
        assert!(cherry_pick_commit(&repo_dir, &pick_oid).is_ok());
        assert!(repo_dir.join("pick.txt").exists());

        let _ = std::fs::remove_dir_all(&repo_dir);
    }

    #[test]
    fn merge_abort_clears_conflict_state() {
        let repo_dir = unique_temp_dir();
        assert!(std::fs::create_dir_all(&repo_dir).is_ok());
        assert!(run_git(&repo_dir, &["init"]).is_ok());
        assert!(run_git(&repo_dir, &["config", "user.email", "dev@example.com"]).is_ok());
        assert!(run_git(&repo_dir, &["config", "user.name", "Dev User"]).is_ok());

        let file = repo_dir.join("conflict.txt");
        assert!(std::fs::write(&file, "line\n").is_ok());
        assert!(stage_paths(&repo_dir, &["conflict.txt".to_string()]).is_ok());
        assert!(commit_create(&repo_dir, "base").is_ok());

        assert!(create_branch(&repo_dir, "feature").is_ok());
        assert!(checkout_branch(&repo_dir, "feature").is_ok());
        assert!(std::fs::write(&file, "feature\n").is_ok());
        assert!(stage_paths(&repo_dir, &["conflict.txt".to_string()]).is_ok());
        assert!(commit_create(&repo_dir, "feature change").is_ok());

        assert!(
            checkout_branch(&repo_dir, "main").is_ok()
                || checkout_branch(&repo_dir, "master").is_ok()
        );
        assert!(std::fs::write(&file, "main\n").is_ok());
        assert!(stage_paths(&repo_dir, &["conflict.txt".to_string()]).is_ok());
        assert!(commit_create(&repo_dir, "main change").is_ok());

        let merge = merge_ref(&repo_dir, "feature", MergeMode::NoFastForward);
        assert!(merge.is_err());
        assert!(matches!(
            detect_conflict_state(&repo_dir),
            Ok(Some(ConflictState::Merge))
        ));

        assert!(merge_abort(&repo_dir).is_ok());
        assert!(matches!(detect_conflict_state(&repo_dir), Ok(None)));

        let _ = std::fs::remove_dir_all(&repo_dir);
    }

    #[test]
    fn lists_and_resolves_conflicted_files_with_ours_choice() {
        let repo_dir = unique_temp_dir();
        assert!(std::fs::create_dir_all(&repo_dir).is_ok());
        assert!(run_git(&repo_dir, &["init"]).is_ok());
        assert!(run_git(&repo_dir, &["config", "user.email", "dev@example.com"]).is_ok());
        assert!(run_git(&repo_dir, &["config", "user.name", "Dev User"]).is_ok());

        let file = repo_dir.join("conflict.txt");
        assert!(std::fs::write(&file, "line\n").is_ok());
        assert!(stage_paths(&repo_dir, &["conflict.txt".to_string()]).is_ok());
        assert!(commit_create(&repo_dir, "base").is_ok());

        assert!(create_branch(&repo_dir, "feature").is_ok());
        assert!(checkout_branch(&repo_dir, "feature").is_ok());
        assert!(std::fs::write(&file, "feature\n").is_ok());
        assert!(stage_paths(&repo_dir, &["conflict.txt".to_string()]).is_ok());
        assert!(commit_create(&repo_dir, "feature change").is_ok());

        assert!(
            checkout_branch(&repo_dir, "main").is_ok()
                || checkout_branch(&repo_dir, "master").is_ok()
        );
        assert!(std::fs::write(&file, "main\n").is_ok());
        assert!(stage_paths(&repo_dir, &["conflict.txt".to_string()]).is_ok());
        assert!(commit_create(&repo_dir, "main change").is_ok());

        assert!(merge_ref(&repo_dir, "feature", MergeMode::NoFastForward).is_err());
        let snapshot = conflict_session_snapshot(&repo_dir)
            .expect("snapshot")
            .expect("active");
        assert!(matches!(snapshot.state, ConflictState::Merge));
        assert!(
            snapshot
                .files
                .iter()
                .any(|item| item.path == "conflict.txt")
        );

        assert!(conflict_apply_choice(&repo_dir, "conflict.txt", ConflictChoice::Ours).is_ok());
        assert!(mark_conflict_resolved(&repo_dir, &["conflict.txt".to_string()]).is_ok());
        assert!(conflict_continue(&repo_dir).is_ok());
        assert!(matches!(detect_conflict_state(&repo_dir), Ok(None)));

        let _ = std::fs::remove_dir_all(&repo_dir);
    }

    #[test]
    fn rebase_continue_uses_noninteractive_editor_fallback() {
        let repo_dir = unique_temp_dir();
        assert!(std::fs::create_dir_all(&repo_dir).is_ok());
        assert!(run_git(&repo_dir, &["init"]).is_ok());
        assert!(run_git(&repo_dir, &["config", "user.email", "dev@example.com"]).is_ok());
        assert!(run_git(&repo_dir, &["config", "user.name", "Dev User"]).is_ok());

        let file = repo_dir.join("rebase.txt");
        assert!(std::fs::write(&file, "base\n").is_ok());
        assert!(stage_paths(&repo_dir, &["rebase.txt".to_string()]).is_ok());
        assert!(commit_create(&repo_dir, "base").is_ok());

        assert!(create_branch(&repo_dir, "topic").is_ok());
        assert!(checkout_branch(&repo_dir, "topic").is_ok());
        assert!(std::fs::write(&file, "topic\n").is_ok());
        assert!(stage_paths(&repo_dir, &["rebase.txt".to_string()]).is_ok());
        assert!(commit_create(&repo_dir, "topic change").is_ok());

        let base = if checkout_branch(&repo_dir, "main").is_ok() {
            "main"
        } else {
            assert!(checkout_branch(&repo_dir, "master").is_ok());
            "master"
        };
        assert!(std::fs::write(&file, "main\n").is_ok());
        assert!(stage_paths(&repo_dir, &["rebase.txt".to_string()]).is_ok());
        assert!(commit_create(&repo_dir, "main change").is_ok());

        assert!(checkout_branch(&repo_dir, "topic").is_ok());
        let plan = create_rebase_plan(&repo_dir, base).expect("plan");
        assert!(execute_rebase_plan(&repo_dir, &plan, false).is_err());
        assert!(matches!(
            detect_conflict_state(&repo_dir),
            Ok(Some(ConflictState::Rebase))
        ));

        assert!(conflict_apply_choice(&repo_dir, "rebase.txt", ConflictChoice::Theirs).is_ok());
        assert!(mark_conflict_resolved(&repo_dir, &["rebase.txt".to_string()]).is_ok());
        assert!(rebase_continue(&repo_dir).is_ok());
        assert!(matches!(detect_conflict_state(&repo_dir), Ok(None)));

        let _ = std::fs::remove_dir_all(&repo_dir);
    }

    #[test]
    fn cherry_pick_continue_uses_noninteractive_editor_fallback() {
        let repo_dir = unique_temp_dir();
        assert!(std::fs::create_dir_all(&repo_dir).is_ok());
        assert!(run_git(&repo_dir, &["init"]).is_ok());
        assert!(run_git(&repo_dir, &["config", "user.email", "dev@example.com"]).is_ok());
        assert!(run_git(&repo_dir, &["config", "user.name", "Dev User"]).is_ok());

        let file = repo_dir.join("pick.txt");
        assert!(std::fs::write(&file, "base\n").is_ok());
        assert!(stage_paths(&repo_dir, &["pick.txt".to_string()]).is_ok());
        assert!(commit_create(&repo_dir, "base").is_ok());

        assert!(create_branch(&repo_dir, "feature").is_ok());
        assert!(checkout_branch(&repo_dir, "feature").is_ok());
        assert!(std::fs::write(&file, "feature\n").is_ok());
        assert!(stage_paths(&repo_dir, &["pick.txt".to_string()]).is_ok());
        assert!(commit_create(&repo_dir, "feature change").is_ok());
        let pick_oid = commit_log_page(&repo_dir, 0, 1)
            .ok()
            .and_then(|mut page| page.pop())
            .map(|item| item.oid)
            .unwrap_or_default();
        assert!(!pick_oid.is_empty());

        assert!(
            checkout_branch(&repo_dir, "main").is_ok()
                || checkout_branch(&repo_dir, "master").is_ok()
        );
        assert!(std::fs::write(&file, "main\n").is_ok());
        assert!(stage_paths(&repo_dir, &["pick.txt".to_string()]).is_ok());
        assert!(commit_create(&repo_dir, "main change").is_ok());

        assert!(cherry_pick_commit(&repo_dir, &pick_oid).is_err());
        assert!(matches!(
            detect_conflict_state(&repo_dir),
            Ok(Some(ConflictState::CherryPick))
        ));

        assert!(conflict_apply_choice(&repo_dir, "pick.txt", ConflictChoice::Theirs).is_ok());
        assert!(mark_conflict_resolved(&repo_dir, &["pick.txt".to_string()]).is_ok());
        assert!(cherry_pick_continue(&repo_dir).is_ok());
        assert!(matches!(detect_conflict_state(&repo_dir), Ok(None)));

        let _ = std::fs::remove_dir_all(&repo_dir);
    }

    #[test]
    fn conflict_abort_dispatches_for_merge_session() {
        let repo_dir = unique_temp_dir();
        assert!(std::fs::create_dir_all(&repo_dir).is_ok());
        assert!(run_git(&repo_dir, &["init"]).is_ok());
        assert!(run_git(&repo_dir, &["config", "user.email", "dev@example.com"]).is_ok());
        assert!(run_git(&repo_dir, &["config", "user.name", "Dev User"]).is_ok());

        let file = repo_dir.join("conflict.txt");
        assert!(std::fs::write(&file, "line\n").is_ok());
        assert!(stage_paths(&repo_dir, &["conflict.txt".to_string()]).is_ok());
        assert!(commit_create(&repo_dir, "base").is_ok());

        assert!(create_branch(&repo_dir, "feature").is_ok());
        assert!(checkout_branch(&repo_dir, "feature").is_ok());
        assert!(std::fs::write(&file, "feature\n").is_ok());
        assert!(stage_paths(&repo_dir, &["conflict.txt".to_string()]).is_ok());
        assert!(commit_create(&repo_dir, "feature change").is_ok());

        assert!(
            checkout_branch(&repo_dir, "main").is_ok()
                || checkout_branch(&repo_dir, "master").is_ok()
        );
        assert!(std::fs::write(&file, "main\n").is_ok());
        assert!(stage_paths(&repo_dir, &["conflict.txt".to_string()]).is_ok());
        assert!(commit_create(&repo_dir, "main change").is_ok());

        assert!(merge_ref(&repo_dir, "feature", MergeMode::NoFastForward).is_err());
        assert!(matches!(
            detect_conflict_state(&repo_dir),
            Ok(Some(ConflictState::Merge))
        ));
        assert!(conflict_abort(&repo_dir).is_ok());
        assert!(matches!(detect_conflict_state(&repo_dir), Ok(None)));

        let _ = std::fs::remove_dir_all(&repo_dir);
    }

    #[test]
    fn creates_rebase_plan_with_autosquash_awareness() {
        let repo_dir = unique_temp_dir();
        assert!(std::fs::create_dir_all(&repo_dir).is_ok());
        assert!(run_git(&repo_dir, &["init"]).is_ok());
        assert!(run_git(&repo_dir, &["config", "user.email", "dev@example.com"]).is_ok());
        assert!(run_git(&repo_dir, &["config", "user.name", "Dev User"]).is_ok());

        assert!(std::fs::write(repo_dir.join("one.txt"), "one\n").is_ok());
        assert!(stage_paths(&repo_dir, &["one.txt".to_string()]).is_ok());
        assert!(commit_create(&repo_dir, "feat: one").is_ok());
        assert!(std::fs::write(repo_dir.join("two.txt"), "two\n").is_ok());
        assert!(stage_paths(&repo_dir, &["two.txt".to_string()]).is_ok());
        assert!(commit_create(&repo_dir, "fixup! feat: one").is_ok());

        let base = run_git(&repo_dir, &["rev-list", "--max-parents=0", "HEAD"])
            .ok()
            .and_then(|out| String::from_utf8(out.stdout).ok())
            .unwrap_or_default()
            .trim()
            .to_string();
        let plan = create_rebase_plan(&repo_dir, &base);
        assert!(plan.is_ok());
        if let Ok(plan) = plan {
            assert_eq!(plan.base_ref, base);
            assert_eq!(plan.entries.len(), 1);
            assert!(plan.autosquash_aware);
        }

        let _ = std::fs::remove_dir_all(&repo_dir);
    }

    #[test]
    fn stash_create_list_apply_pop_drop_flow() {
        let repo_dir = unique_temp_dir();
        assert!(std::fs::create_dir_all(&repo_dir).is_ok());
        assert!(run_git(&repo_dir, &["init"]).is_ok());
        assert!(run_git(&repo_dir, &["config", "user.email", "dev@example.com"]).is_ok());
        assert!(run_git(&repo_dir, &["config", "user.name", "Dev User"]).is_ok());

        let file = repo_dir.join("stash.txt");
        assert!(std::fs::write(&file, "base\n").is_ok());
        assert!(stage_paths(&repo_dir, &["stash.txt".to_string()]).is_ok());
        assert!(commit_create(&repo_dir, "base").is_ok());

        assert!(std::fs::write(&file, "work\n").is_ok());
        assert!(stash_create(&repo_dir, Some("wip")).is_ok());
        let list = stash_list(&repo_dir).expect("stash list");
        assert!(!list.is_empty());

        let reference = list[0].reference.clone();
        assert!(stash_apply(&repo_dir, &reference).is_ok());
        let applied = std::fs::read_to_string(&file).unwrap_or_default();
        assert!(applied.contains("work"));

        assert!(stash_drop(&repo_dir, &reference).is_ok());

        assert!(std::fs::write(&file, "next\n").is_ok());
        assert!(stash_create(&repo_dir, Some("next")).is_ok());
        assert!(stash_pop(&repo_dir, None).is_ok());

        let _ = std::fs::remove_dir_all(&repo_dir);
    }

    #[test]
    fn file_history_and_blame_baseline() {
        let repo_dir = unique_temp_dir();
        assert!(std::fs::create_dir_all(&repo_dir).is_ok());
        assert!(run_git(&repo_dir, &["init"]).is_ok());
        assert!(run_git(&repo_dir, &["config", "user.email", "dev@example.com"]).is_ok());
        assert!(run_git(&repo_dir, &["config", "user.name", "Dev User"]).is_ok());

        let file = "tracked.txt".to_string();
        assert!(std::fs::write(repo_dir.join(&file), "line\n").is_ok());
        assert!(stage_paths(&repo_dir, std::slice::from_ref(&file)).is_ok());
        assert!(commit_create(&repo_dir, "feat: tracked").is_ok());

        assert!(std::fs::write(repo_dir.join(&file), "line\nnext\n").is_ok());
        assert!(stage_paths(&repo_dir, std::slice::from_ref(&file)).is_ok());
        assert!(commit_create(&repo_dir, "feat: tracked 2").is_ok());

        let history = file_history_page(&repo_dir, &file, 0, 10).expect("file history");
        assert!(history.len() >= 2);

        let blame = file_blame(&repo_dir, &file, None).expect("blame");
        assert!(blame.iter().any(|line| line.content.contains("line")));

        let prefix = history[0].oid.chars().take(7).collect::<String>();
        let filtered =
            commit_log_page_filtered_with_hash_prefix(&repo_dir, 0, 10, None, None, Some(&prefix))
                .expect("hash filter");
        assert!(!filtered.is_empty());
        assert!(filtered.iter().all(|item| item.oid.starts_with(&prefix)));

        let _ = std::fs::remove_dir_all(&repo_dir);
    }

    #[test]
    fn executes_rebase_plan_reorder_and_drop() {
        let repo_dir = unique_temp_dir();
        assert!(std::fs::create_dir_all(&repo_dir).is_ok());
        assert!(run_git(&repo_dir, &["init"]).is_ok());
        assert!(run_git(&repo_dir, &["config", "user.email", "dev@example.com"]).is_ok());
        assert!(run_git(&repo_dir, &["config", "user.name", "Dev User"]).is_ok());

        assert!(std::fs::write(repo_dir.join("one.txt"), "one\n").is_ok());
        assert!(stage_paths(&repo_dir, &["one.txt".to_string()]).is_ok());
        assert!(commit_create(&repo_dir, "feat: one").is_ok());
        assert!(std::fs::write(repo_dir.join("two.txt"), "two\n").is_ok());
        assert!(stage_paths(&repo_dir, &["two.txt".to_string()]).is_ok());
        assert!(commit_create(&repo_dir, "feat: two").is_ok());
        assert!(std::fs::write(repo_dir.join("three.txt"), "three\n").is_ok());
        assert!(stage_paths(&repo_dir, &["three.txt".to_string()]).is_ok());
        assert!(commit_create(&repo_dir, "feat: three").is_ok());

        let base = run_git(&repo_dir, &["rev-list", "--max-parents=0", "HEAD"])
            .ok()
            .and_then(|out| String::from_utf8(out.stdout).ok())
            .unwrap_or_default()
            .trim()
            .to_string();
        let mut plan = create_rebase_plan(&repo_dir, &base).expect("plan");
        assert_eq!(plan.entries.len(), 2);

        plan.entries.swap(0, 1);
        if let Some(last) = plan.entries.last_mut() {
            last.action = RebaseAction::Drop;
        }

        let executed = execute_rebase_plan(&repo_dir, &plan, false);
        assert!(executed.is_ok());
        let log = commit_log_page(&repo_dir, 0, 10).expect("log");
        let summaries = log.into_iter().map(|c| c.summary).collect::<Vec<_>>();
        assert_eq!(plan.entries.len(), 2);
        assert!(summaries.iter().any(|s| s == "feat: two"));
        assert!(!summaries.iter().any(|s| s == "feat: three"));

        let _ = std::fs::remove_dir_all(&repo_dir);
    }

    #[test]
    fn detects_rebase_restart_hook() {
        let repo_dir = unique_temp_dir();
        assert!(std::fs::create_dir_all(&repo_dir).is_ok());
        assert!(run_git(&repo_dir, &["init"]).is_ok());

        let rebase_merge = git_path(&repo_dir, "rebase-merge").expect("git path");
        assert!(std::fs::create_dir_all(&rebase_merge).is_ok());
        assert!(std::fs::write(rebase_merge.join("msgnum"), "2\n").is_ok());
        assert!(std::fs::write(rebase_merge.join("end"), "5\n").is_ok());

        let detected = detect_rebase_session_hook(&repo_dir);
        assert!(detected.is_ok());
        if let Ok(Some(hook)) = detected {
            assert!(hook.active);
            assert_eq!(hook.current_step, Some(2));
            assert_eq!(hook.total_steps, Some(5));
        }

        let _ = std::fs::remove_dir_all(&repo_dir);
    }

    #[test]
    fn detects_rebase_restart_hook_for_rebase_apply_layout() {
        let repo_dir = unique_temp_dir();
        assert!(std::fs::create_dir_all(&repo_dir).is_ok());
        assert!(run_git(&repo_dir, &["init"]).is_ok());

        let rebase_apply = git_path(&repo_dir, "rebase-apply").expect("git path");
        assert!(std::fs::create_dir_all(&rebase_apply).is_ok());
        assert!(std::fs::write(rebase_apply.join("next"), "3\n").is_ok());
        assert!(std::fs::write(rebase_apply.join("last"), "7\n").is_ok());

        let detected = detect_rebase_session_hook(&repo_dir);
        assert!(detected.is_ok());
        if let Ok(Some(hook)) = detected {
            assert!(hook.active);
            assert_eq!(hook.current_step, Some(3));
            assert_eq!(hook.total_steps, Some(7));
        }

        let _ = std::fs::remove_dir_all(&repo_dir);
    }
}
