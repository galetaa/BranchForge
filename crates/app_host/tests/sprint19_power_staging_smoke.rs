use job_system::{JobLock, JobRequest, execute_job_op};
use state_store::StateStore;

fn unique_temp_dir() -> std::path::PathBuf {
    static COUNTER: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0);
    let seq = COUNTER.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    std::env::temp_dir().join(format!("branchforge-sprint19-smoke-{nanos}-{seq}"))
}

#[test]
fn sprint19_hunk_stage_unstage_discard_flow() {
    let repo_dir = unique_temp_dir();
    assert!(std::fs::create_dir_all(&repo_dir).is_ok());
    assert!(git_service::run_git(&repo_dir, &["init"]).is_ok());
    assert!(git_service::run_git(&repo_dir, &["config", "user.email", "dev@example.com"]).is_ok());
    assert!(git_service::run_git(&repo_dir, &["config", "user.name", "Dev User"]).is_ok());

    let file = repo_dir.join("hunks.txt");
    let mut lines = Vec::new();
    for idx in 1..=20 {
        lines.push(format!("line{idx}\n"));
    }
    assert!(std::fs::write(&file, lines.concat()).is_ok());
    assert!(git_service::stage_paths(&repo_dir, &["hunks.txt".to_string()]).is_ok());
    assert!(git_service::commit_create(&repo_dir, "base").is_ok());

    lines[0] = "line1-updated\n".to_string();
    lines[19] = "line20-updated\n".to_string();
    assert!(std::fs::write(&file, lines.concat()).is_ok());

    let mut store = StateStore::new();
    assert!(
        execute_job_op(
            &repo_dir,
            &JobRequest {
                op: "diff.worktree".to_string(),
                lock: JobLock::Read,
                paths: vec!["hunks.txt".to_string()],
                job_id: None,
            },
            &mut store,
        )
        .is_ok()
    );
    assert!(store.snapshot().diff.hunks.len() >= 2);

    assert!(
        execute_job_op(
            &repo_dir,
            &JobRequest {
                op: "index.stage_hunk".to_string(),
                lock: JobLock::IndexWrite,
                paths: vec!["hunks.txt".to_string(), "0".to_string()],
                job_id: None,
            },
            &mut store,
        )
        .is_ok()
    );
    assert!(
        store
            .snapshot()
            .status
            .staged
            .iter()
            .any(|path| path == "hunks.txt")
    );

    assert!(
        execute_job_op(
            &repo_dir,
            &JobRequest {
                op: "index.unstage_hunk".to_string(),
                lock: JobLock::IndexWrite,
                paths: vec!["hunks.txt".to_string(), "0".to_string()],
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
                op: "file.discard_hunk".to_string(),
                lock: JobLock::IndexWrite,
                paths: vec!["hunks.txt".to_string(), "0".to_string()],
                job_id: None,
            },
            &mut store,
        )
        .is_ok()
    );

    let content = std::fs::read_to_string(&file).unwrap_or_default();
    assert!(content.contains("line1\n"));
    assert!(content.contains("line20-updated\n"));

    let _ = std::fs::remove_dir_all(&repo_dir);
}
