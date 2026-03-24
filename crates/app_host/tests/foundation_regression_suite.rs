use app_host::errors::translate_job_error;
use git_service::{ConflictState, GitServiceError};
use job_system::{JobExecutionError, JobLock, JobRequest, execute_job_op};
use state_store::StateStore;

fn unique_temp_dir(label: &str) -> std::path::PathBuf {
    static COUNTER: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0);
    let seq = COUNTER.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    std::env::temp_dir().join(format!("branchforge-foundation-{label}-{nanos}-{seq}"))
}

fn init_repo() -> std::path::PathBuf {
    let repo_dir = unique_temp_dir("repo");
    assert!(std::fs::create_dir_all(&repo_dir).is_ok());
    assert!(git_service::run_git(&repo_dir, &["init"]).is_ok());
    assert!(git_service::run_git(&repo_dir, &["config", "user.email", "dev@example.com"]).is_ok());
    assert!(git_service::run_git(&repo_dir, &["config", "user.name", "Dev User"]).is_ok());
    repo_dir
}

#[test]
fn compare_refs_updates_diff_and_compare_state() {
    let repo_dir = init_repo();
    let file = repo_dir.join("compare.txt");
    assert!(std::fs::write(&file, "base\n").is_ok());
    assert!(git_service::stage_paths(&repo_dir, &["compare.txt".to_string()]).is_ok());
    assert!(git_service::commit_create(&repo_dir, "base commit").is_ok());

    let base_ref = git_service::repo_open(&repo_dir)
        .ok()
        .and_then(|repo| repo.head)
        .unwrap_or_else(|| "main".to_string());

    assert!(git_service::create_branch(&repo_dir, "feature").is_ok());
    assert!(git_service::checkout_branch(&repo_dir, "feature").is_ok());
    assert!(std::fs::write(&file, "base\nfeature\n").is_ok());
    assert!(git_service::stage_paths(&repo_dir, &["compare.txt".to_string()]).is_ok());
    assert!(git_service::commit_create(&repo_dir, "feature commit").is_ok());

    assert!(git_service::checkout_branch(&repo_dir, &base_ref).is_ok());

    let mut store = StateStore::new();
    let result = execute_job_op(
        &repo_dir,
        &JobRequest {
            op: "compare.refs".to_string(),
            lock: JobLock::Read,
            paths: vec![base_ref.clone(), "feature".to_string()],
            job_id: None,
        },
        &mut store,
    );
    assert!(result.is_ok());
    assert_eq!(
        store.snapshot().compare.base_ref.as_deref(),
        Some(base_ref.as_str())
    );
    assert_eq!(
        store.snapshot().compare.head_ref.as_deref(),
        Some("feature")
    );
    assert_eq!(store.snapshot().compare.ahead, 1);
    assert_eq!(store.snapshot().compare.behind, 0);
    assert_eq!(store.snapshot().compare.commits.len(), 1);
    assert!(
        store
            .snapshot()
            .diff
            .content
            .as_deref()
            .unwrap_or("")
            .contains("feature")
    );

    let _ = std::fs::remove_dir_all(&repo_dir);
}

#[test]
fn file_discard_restores_worktree_file() {
    let repo_dir = init_repo();
    let file = repo_dir.join("discard.txt");
    assert!(std::fs::write(&file, "base\n").is_ok());
    assert!(git_service::stage_paths(&repo_dir, &["discard.txt".to_string()]).is_ok());
    assert!(git_service::commit_create(&repo_dir, "base commit").is_ok());
    assert!(std::fs::write(&file, "mutated\n").is_ok());

    let mut store = StateStore::new();
    let result = execute_job_op(
        &repo_dir,
        &JobRequest {
            op: "file.discard".to_string(),
            lock: JobLock::IndexWrite,
            paths: vec!["discard.txt".to_string()],
            job_id: None,
        },
        &mut store,
    );

    assert!(result.is_ok());
    let content = std::fs::read_to_string(&file).unwrap_or_default();
    assert_eq!(content, "base\n");

    let _ = std::fs::remove_dir_all(&repo_dir);
}

#[test]
fn tag_delete_removes_existing_tag() {
    let repo_dir = init_repo();
    let file = repo_dir.join("tag-delete.txt");
    assert!(std::fs::write(&file, "base\n").is_ok());
    assert!(git_service::stage_paths(&repo_dir, &["tag-delete.txt".to_string()]).is_ok());
    assert!(git_service::commit_create(&repo_dir, "base commit").is_ok());
    assert!(git_service::create_tag(&repo_dir, "v0.3.0").is_ok());

    let mut store = StateStore::new();
    let open = execute_job_op(
        &repo_dir,
        &JobRequest {
            op: "repo.open".to_string(),
            lock: JobLock::Read,
            paths: Vec::new(),
            job_id: None,
        },
        &mut store,
    );
    assert!(open.is_ok());

    let result = execute_job_op(
        &repo_dir,
        &JobRequest {
            op: "tag.delete".to_string(),
            lock: JobLock::RefsWrite,
            paths: vec!["v0.3.0".to_string()],
            job_id: None,
        },
        &mut store,
    );

    assert!(result.is_ok());
    assert!(
        !store
            .snapshot()
            .tags
            .tags
            .iter()
            .any(|tag| tag.name == "v0.3.0")
    );

    let _ = std::fs::remove_dir_all(&repo_dir);
}

#[test]
fn conflict_state_blocks_checkout() {
    let repo_dir = init_repo();
    let file = repo_dir.join("conflict.txt");
    assert!(std::fs::write(&file, "line\n").is_ok());
    assert!(git_service::stage_paths(&repo_dir, &["conflict.txt".to_string()]).is_ok());
    assert!(git_service::commit_create(&repo_dir, "base").is_ok());

    let base_ref = git_service::repo_open(&repo_dir)
        .ok()
        .and_then(|repo| repo.head)
        .unwrap_or_else(|| "main".to_string());

    assert!(git_service::create_branch(&repo_dir, "feature").is_ok());
    assert!(git_service::checkout_branch(&repo_dir, "feature").is_ok());
    assert!(std::fs::write(&file, "feature\n").is_ok());
    assert!(git_service::stage_paths(&repo_dir, &["conflict.txt".to_string()]).is_ok());
    assert!(git_service::commit_create(&repo_dir, "feature change").is_ok());

    assert!(git_service::checkout_branch(&repo_dir, &base_ref).is_ok());
    assert!(std::fs::write(&file, "base\n").is_ok());
    assert!(git_service::stage_paths(&repo_dir, &["conflict.txt".to_string()]).is_ok());
    assert!(git_service::commit_create(&repo_dir, "base change").is_ok());

    let merge = git_service::run_git(&repo_dir, &["merge", "feature"]);
    assert!(merge.is_err());

    let mut state = git_service::detect_conflict_state(&repo_dir).expect("state");
    if state.is_none() {
        let merge_head =
            git_service::run_git(&repo_dir, &["rev-parse", "--git-path", "MERGE_HEAD"])
                .ok()
                .and_then(|out| String::from_utf8(out.stdout).ok())
                .map(|text| std::path::PathBuf::from(text.trim().to_string()));
        if let Some(path) = merge_head {
            let _ = std::fs::write(path, "dummy");
        }
        state = git_service::detect_conflict_state(&repo_dir).expect("state");
    }
    assert!(matches!(
        state,
        Some(ConflictState::Merge | ConflictState::Rebase | ConflictState::CherryPick)
    ));

    let mut store = StateStore::new();
    let result = execute_job_op(
        &repo_dir,
        &JobRequest {
            op: "branch.checkout".to_string(),
            lock: JobLock::RefsWrite,
            paths: vec![base_ref.clone()],
            job_id: None,
        },
        &mut store,
    );
    assert!(matches!(
        result,
        Err(JobExecutionError::InvalidInput { message }) if message.contains("merge state")
    ));

    let translated = translate_job_error(&JobExecutionError::Git(GitServiceError::GitError {
        exit_code: 1,
        stderr: "error: you have unmerged files".to_string(),
    }));
    assert_eq!(translated.title, "Unresolved conflicts");

    let _ = std::fs::remove_dir_all(&repo_dir);
}
