use job_system::{JobLock, JobRequest, execute_job_op};
use state_store::StateStore;

fn unique_temp_dir() -> std::path::PathBuf {
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0);
    std::env::temp_dir().join(format!("branchforge-job-system-integration-{nanos}"))
}

#[test]
fn repo_open_then_status_refresh_updates_snapshots() {
    let repo_dir = unique_temp_dir();
    assert!(std::fs::create_dir_all(&repo_dir).is_ok());
    assert!(git_service::run_git(&repo_dir, &["init"]).is_ok());

    // First file is visible during repo.open refresh.
    assert!(std::fs::write(repo_dir.join("README.md"), "hello\n").is_ok());

    let mut store = StateStore::new();
    let open_req = JobRequest {
        op: "repo.open".to_string(),
        lock: JobLock::Read,
        paths: Vec::new(),
    };
    let open_result = execute_job_op(&repo_dir, &open_req, &mut store);
    assert!(open_result.is_ok());

    let open_snapshot = store.snapshot().clone();
    assert!(open_snapshot.repo.is_some());
    assert!(
        open_snapshot
            .status
            .untracked
            .iter()
            .any(|p| p == "README.md")
    );

    // New file appears after explicit status.refresh.
    assert!(std::fs::write(repo_dir.join("notes.txt"), "note\n").is_ok());
    let refresh_req = JobRequest {
        op: "status.refresh".to_string(),
        lock: JobLock::Read,
        paths: Vec::new(),
    };
    let refresh_result = execute_job_op(&repo_dir, &refresh_req, &mut store);
    assert!(refresh_result.is_ok());

    let refreshed = store.snapshot();
    assert!(refreshed.status.untracked.iter().any(|p| p == "notes.txt"));
    assert!(refreshed.version > open_snapshot.version);

    let _ = std::fs::remove_dir_all(&repo_dir);
}
