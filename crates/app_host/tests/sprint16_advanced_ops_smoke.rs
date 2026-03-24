use job_system::{JobLock, JobRequest, execute_job_op};
use state_store::StateStore;

fn unique_temp_dir(label: &str) -> std::path::PathBuf {
    static COUNTER: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0);
    let seq = COUNTER.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    std::env::temp_dir().join(format!("branchforge-sprint16-{label}-{nanos}-{seq}"))
}

fn init_repo() -> std::path::PathBuf {
    let repo_dir = unique_temp_dir("repo");
    assert!(std::fs::create_dir_all(&repo_dir).is_ok());
    assert!(git_service::run_git(&repo_dir, &["init"]).is_ok());
    assert!(git_service::run_git(&repo_dir, &["config", "user.email", "dev@example.com"]).is_ok());
    assert!(git_service::run_git(&repo_dir, &["config", "user.name", "Dev User"]).is_ok());
    repo_dir
}

fn default_branch(repo_dir: &std::path::Path) -> String {
    git_service::repo_open(repo_dir)
        .ok()
        .and_then(|repo| repo.head)
        .unwrap_or_else(|| "main".to_string())
}

#[test]
fn merge_and_revert_and_reset_hard_work_end_to_end() {
    let repo_dir = init_repo();
    let file = repo_dir.join("advanced.txt");

    assert!(std::fs::write(&file, "base\n").is_ok());
    assert!(git_service::stage_paths(&repo_dir, &["advanced.txt".to_string()]).is_ok());
    assert!(git_service::commit_create(&repo_dir, "base").is_ok());

    let base = default_branch(&repo_dir);
    assert!(git_service::create_branch(&repo_dir, "feature").is_ok());
    assert!(git_service::checkout_branch(&repo_dir, "feature").is_ok());
    assert!(std::fs::write(&file, "base\nfeature\n").is_ok());
    assert!(git_service::stage_paths(&repo_dir, &["advanced.txt".to_string()]).is_ok());
    assert!(git_service::commit_create(&repo_dir, "feature commit").is_ok());
    assert!(git_service::checkout_branch(&repo_dir, &base).is_ok());

    let mut store = StateStore::new();
    let merge = execute_job_op(
        &repo_dir,
        &JobRequest {
            op: "merge.execute".to_string(),
            lock: JobLock::RefsWrite,
            paths: vec!["feature".to_string(), "ff".to_string()],
            job_id: None,
        },
        &mut store,
    );
    assert!(merge.is_ok());
    let merged = std::fs::read_to_string(&file).unwrap_or_default();
    assert!(merged.contains("feature"));

    assert!(std::fs::write(&file, "base\nfeature\nlocal\n").is_ok());
    assert!(git_service::stage_paths(&repo_dir, &["advanced.txt".to_string()]).is_ok());
    assert!(git_service::commit_create(&repo_dir, "local commit").is_ok());
    let revert_oid = git_service::commit_log_page(&repo_dir, 0, 1)
        .ok()
        .and_then(|mut page| page.pop())
        .map(|item| item.oid)
        .unwrap_or_default();

    let revert = execute_job_op(
        &repo_dir,
        &JobRequest {
            op: "revert.commit".to_string(),
            lock: JobLock::RefsWrite,
            paths: vec![revert_oid],
            job_id: None,
        },
        &mut store,
    );
    assert!(revert.is_ok());

    assert!(std::fs::write(&file, "dirty\n").is_ok());
    let reset = execute_job_op(
        &repo_dir,
        &JobRequest {
            op: "reset.refs".to_string(),
            lock: JobLock::RefsWrite,
            paths: vec!["hard".to_string(), "HEAD".to_string()],
            job_id: None,
        },
        &mut store,
    );
    assert!(reset.is_ok());
    let reset_content = std::fs::read_to_string(&file).unwrap_or_default();
    assert!(!reset_content.contains("dirty"));

    let merge_entry = store
        .snapshot()
        .journal
        .entries
        .iter()
        .find(|entry| entry.op == "merge.execute");
    assert!(merge_entry.is_some());
    if let Some(entry) = merge_entry {
        assert!(entry.session_id.is_some());
        assert!(matches!(
            entry.session_state,
            Some(state_store::OperationSessionState::Succeeded)
        ));
        assert!(entry.pre_refs.is_some());
        assert!(entry.post_refs.is_some());
    }

    let _ = std::fs::remove_dir_all(&repo_dir);
}

#[test]
fn cherry_pick_and_merge_abort_flow_work_end_to_end() {
    let repo_dir = init_repo();
    let conflict_file = repo_dir.join("conflict.txt");
    assert!(std::fs::write(&conflict_file, "line\n").is_ok());
    assert!(git_service::stage_paths(&repo_dir, &["conflict.txt".to_string()]).is_ok());
    assert!(git_service::commit_create(&repo_dir, "base").is_ok());

    let base = default_branch(&repo_dir);

    assert!(git_service::create_branch(&repo_dir, "feature").is_ok());
    assert!(git_service::checkout_branch(&repo_dir, "feature").is_ok());
    assert!(std::fs::write(&conflict_file, "feature\n").is_ok());
    assert!(git_service::stage_paths(&repo_dir, &["conflict.txt".to_string()]).is_ok());
    assert!(git_service::commit_create(&repo_dir, "feature change").is_ok());

    assert!(git_service::create_branch(&repo_dir, "pick-source").is_ok());
    assert!(git_service::checkout_branch(&repo_dir, "pick-source").is_ok());
    assert!(std::fs::write(repo_dir.join("pick.txt"), "picked\n").is_ok());
    assert!(git_service::stage_paths(&repo_dir, &["pick.txt".to_string()]).is_ok());
    assert!(git_service::commit_create(&repo_dir, "pick commit").is_ok());
    let pick_oid = git_service::commit_log_page(&repo_dir, 0, 1)
        .ok()
        .and_then(|mut page| page.pop())
        .map(|item| item.oid)
        .unwrap_or_default();

    assert!(git_service::checkout_branch(&repo_dir, &base).is_ok());
    assert!(std::fs::write(&conflict_file, "main\n").is_ok());
    assert!(git_service::stage_paths(&repo_dir, &["conflict.txt".to_string()]).is_ok());
    assert!(git_service::commit_create(&repo_dir, "main change").is_ok());

    let mut store = StateStore::new();
    let merge = execute_job_op(
        &repo_dir,
        &JobRequest {
            op: "merge.execute".to_string(),
            lock: JobLock::RefsWrite,
            paths: vec!["feature".to_string(), "no-ff".to_string()],
            job_id: None,
        },
        &mut store,
    );
    assert!(merge.is_err());
    assert!(matches!(
        store.snapshot().repo.as_ref().and_then(|repo| repo.conflict_state.clone()),
        Some(plugin_api::ConflictState::Merge)
    ));

    let abort = execute_job_op(
        &repo_dir,
        &JobRequest {
            op: "merge.abort".to_string(),
            lock: JobLock::RefsWrite,
            paths: Vec::new(),
            job_id: None,
        },
        &mut store,
    );
    assert!(abort.is_ok());
    assert!(
        store
            .snapshot()
            .repo
            .as_ref()
            .and_then(|repo| repo.conflict_state.clone())
            .is_none()
    );

    let cherry = execute_job_op(
        &repo_dir,
        &JobRequest {
            op: "cherry_pick.commit".to_string(),
            lock: JobLock::RefsWrite,
            paths: vec![pick_oid],
            job_id: None,
        },
        &mut store,
    );
    assert!(cherry.is_ok());
    assert!(repo_dir.join("pick.txt").exists());

    let merge_entry = store
        .snapshot()
        .journal
        .entries
        .iter()
        .find(|entry| entry.op == "merge.execute");
    assert!(merge_entry.is_some());
    if let Some(entry) = merge_entry {
        assert!(matches!(
            entry.session_state,
            Some(state_store::OperationSessionState::Failed)
        ));
        assert!(entry.pre_refs.is_some());
        assert!(entry.post_refs.is_some());
    }

    let _ = std::fs::remove_dir_all(&repo_dir);
}


