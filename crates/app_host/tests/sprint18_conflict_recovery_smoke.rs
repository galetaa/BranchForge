use job_system::{JobLock, JobRequest, execute_job_op};
use state_store::StateStore;

fn unique_temp_dir() -> std::path::PathBuf {
    static COUNTER: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0);
    let seq = COUNTER.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    std::env::temp_dir().join(format!("branchforge-sprint18-smoke-{nanos}-{seq}"))
}

#[test]
fn sprint18_merge_conflict_recovery_flow() {
    let repo_dir = unique_temp_dir();
    assert!(std::fs::create_dir_all(&repo_dir).is_ok());
    assert!(git_service::run_git(&repo_dir, &["init"]).is_ok());
    assert!(git_service::run_git(&repo_dir, &["config", "user.email", "dev@example.com"]).is_ok());
    assert!(git_service::run_git(&repo_dir, &["config", "user.name", "Dev User"]).is_ok());

    let file = repo_dir.join("conflict.txt");
    assert!(std::fs::write(&file, "line\n").is_ok());
    assert!(git_service::stage_paths(&repo_dir, &["conflict.txt".to_string()]).is_ok());
    assert!(git_service::commit_create(&repo_dir, "base").is_ok());

    assert!(git_service::create_branch(&repo_dir, "feature").is_ok());
    assert!(git_service::checkout_branch(&repo_dir, "feature").is_ok());
    assert!(std::fs::write(&file, "feature\n").is_ok());
    assert!(git_service::stage_paths(&repo_dir, &["conflict.txt".to_string()]).is_ok());
    assert!(git_service::commit_create(&repo_dir, "feature change").is_ok());

    assert!(
        git_service::checkout_branch(&repo_dir, "main").is_ok()
            || git_service::checkout_branch(&repo_dir, "master").is_ok()
    );
    assert!(std::fs::write(&file, "main\n").is_ok());
    assert!(git_service::stage_paths(&repo_dir, &["conflict.txt".to_string()]).is_ok());
    assert!(git_service::commit_create(&repo_dir, "main change").is_ok());
    assert!(
        git_service::merge_ref(&repo_dir, "feature", git_service::MergeMode::NoFastForward)
            .is_err()
    );

    let mut store = StateStore::new();
    assert!(
        execute_job_op(
            &repo_dir,
            &JobRequest {
                op: "repo.open".to_string(),
                lock: JobLock::Read,
                paths: Vec::new(),
                job_id: None,
            },
            &mut store,
        )
        .is_ok()
    );
    assert!(
        store
            .snapshot()
            .repo
            .as_ref()
            .and_then(|repo| repo.conflict_state.as_ref())
            .is_some()
    );

    assert!(
        execute_job_op(
            &repo_dir,
            &JobRequest {
                op: "conflict.list".to_string(),
                lock: JobLock::Read,
                paths: Vec::new(),
                job_id: None,
            },
            &mut store,
        )
        .is_ok()
    );
    assert!(
        store
            .snapshot()
            .selection
            .selected_paths
            .iter()
            .any(|path| path == "conflict.txt")
    );

    assert!(
        execute_job_op(
            &repo_dir,
            &JobRequest {
                op: "conflict.resolve.ours".to_string(),
                lock: JobLock::IndexWrite,
                paths: vec!["conflict.txt".to_string()],
                job_id: None,
            },
            &mut store,
        )
        .is_ok()
    );

    assert!(
        execute_job_op(
            &repo_dir,
            &JobRequest {
                op: "conflict.mark_resolved".to_string(),
                lock: JobLock::IndexWrite,
                paths: vec!["conflict.txt".to_string()],
                job_id: None,
            },
            &mut store,
        )
        .is_ok()
    );

    assert!(
        execute_job_op(
            &repo_dir,
            &JobRequest {
                op: "conflict.continue".to_string(),
                lock: JobLock::RefsWrite,
                paths: Vec::new(),
                job_id: None,
            },
            &mut store,
        )
        .is_ok()
    );

    assert!(
        store
            .snapshot()
            .repo
            .as_ref()
            .and_then(|repo| repo.conflict_state.as_ref())
            .is_none()
    );
    let _ = std::fs::remove_dir_all(&repo_dir);
}
