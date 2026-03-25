use job_system::{JobLock, JobRequest, execute_job_op};
use state_store::StateStore;

fn unique_temp_dir() -> std::path::PathBuf {
    static COUNTER: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0);
    let seq = COUNTER.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    std::env::temp_dir().join(format!("branchforge-sprint20-smoke-{nanos}-{seq}"))
}

#[test]
fn sprint20_productivity_suite_smoke() {
    let repo_dir = unique_temp_dir();
    assert!(std::fs::create_dir_all(&repo_dir).is_ok());
    assert!(git_service::run_git(&repo_dir, &["init"]).is_ok());
    assert!(git_service::run_git(&repo_dir, &["config", "user.email", "dev@example.com"]).is_ok());
    assert!(git_service::run_git(&repo_dir, &["config", "user.name", "Dev User"]).is_ok());

    let file = repo_dir.join("tracked.txt");
    assert!(std::fs::write(&file, "line\n").is_ok());
    assert!(git_service::stage_paths(&repo_dir, &["tracked.txt".to_string()]).is_ok());
    assert!(git_service::commit_create(&repo_dir, "feat: one").is_ok());

    assert!(std::fs::write(&file, "line\nnext\n").is_ok());
    assert!(git_service::stage_paths(&repo_dir, &["tracked.txt".to_string()]).is_ok());
    assert!(git_service::commit_create(&repo_dir, "feat: two").is_ok());

    assert!(std::fs::write(&file, "line\nnext\nwork\n").is_ok());

    let mut store = StateStore::new();
    assert!(
        execute_job_op(
            &repo_dir,
            &JobRequest {
                op: "stash.create".to_string(),
                lock: JobLock::IndexWrite,
                paths: vec!["wip: sprint20".to_string()],
                job_id: None,
            },
            &mut store,
        )
        .is_ok()
    );

    let list = git_service::stash_list(&repo_dir).expect("stash list");
    assert!(!list.is_empty());
    let stash_ref = list[0].reference.clone();

    assert!(
        execute_job_op(
            &repo_dir,
            &JobRequest {
                op: "stash.apply".to_string(),
                lock: JobLock::IndexWrite,
                paths: vec![stash_ref.clone()],
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
                op: "stash.drop".to_string(),
                lock: JobLock::RefsWrite,
                paths: vec![stash_ref],
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
                op: "history.file".to_string(),
                lock: JobLock::Read,
                paths: vec!["tracked.txt".to_string(), "0".to_string(), "10".to_string()],
                job_id: None,
            },
            &mut store,
        )
        .is_ok()
    );
    assert!(!store.snapshot().history.commits.is_empty());

    let prefix = store.snapshot().history.commits[0]
        .oid
        .chars()
        .take(7)
        .collect::<String>();

    assert!(
        execute_job_op(
            &repo_dir,
            &JobRequest {
                op: "history.search".to_string(),
                lock: JobLock::Read,
                paths: vec![
                    "0".to_string(),
                    "20".to_string(),
                    "".to_string(),
                    "".to_string(),
                    prefix.clone(),
                ],
                job_id: None,
            },
            &mut store,
        )
        .is_ok()
    );
    assert!(
        store
            .snapshot()
            .history
            .commits
            .iter()
            .all(|commit| commit.oid.starts_with(&prefix))
    );

    assert!(
        execute_job_op(
            &repo_dir,
            &JobRequest {
                op: "blame.file".to_string(),
                lock: JobLock::Read,
                paths: vec!["tracked.txt".to_string()],
                job_id: None,
            },
            &mut store,
        )
        .is_ok()
    );
    let diff_text = store.snapshot().diff.content.clone().unwrap_or_default();
    assert!(diff_text.contains("Dev User"));

    let _ = std::fs::remove_dir_all(&repo_dir);
}
