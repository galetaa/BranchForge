use app_host::{run_commit_amend_smoke, run_history_search_smoke};
use job_system::{JobLock, JobRequest, execute_job_op};
use state_store::StateStore;

fn unique_temp_dir(label: &str) -> std::path::PathBuf {
    static COUNTER: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0);
    let seq = COUNTER.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    std::env::temp_dir().join(format!("branchforge-commit-polish-{label}-{nanos}-{seq}"))
}

#[test]
fn commit_amend_and_search_and_tags_smoke() {
    let repo_dir = unique_temp_dir("repo");
    assert!(std::fs::create_dir_all(&repo_dir).is_ok());
    assert!(git_service::run_git(&repo_dir, &["init"]).is_ok());
    assert!(git_service::run_git(&repo_dir, &["config", "user.email", "dev@example.com"]).is_ok());
    assert!(git_service::run_git(&repo_dir, &["config", "user.name", "Dev User"]).is_ok());

    assert!(std::fs::write(repo_dir.join("one.txt"), "one\n").is_ok());
    assert!(git_service::stage_paths(&repo_dir, &["one.txt".to_string()]).is_ok());
    assert!(git_service::commit_create(&repo_dir, "feat: one").is_ok());

    assert!(std::fs::write(repo_dir.join("two.txt"), "two\n").is_ok());
    assert!(git_service::stage_paths(&repo_dir, &["two.txt".to_string()]).is_ok());
    assert!(git_service::commit_create(&repo_dir, "feat: two").is_ok());

    let amend = run_commit_amend_smoke(&repo_dir, Some("feat: two amended".to_string()));
    assert!(amend.is_ok());

    let commits =
        run_history_search_smoke(&repo_dir, None, Some("two amended".to_string())).expect("search");
    assert!(commits.iter().any(|c| c.summary.contains("two amended")));

    let mut store = StateStore::new();
    assert!(
        execute_job_op(
            &repo_dir,
            &JobRequest {
                op: "tag.create".to_string(),
                lock: JobLock::RefsWrite,
                paths: vec!["v0.2.0".to_string()],
            },
            &mut store,
        )
        .is_ok()
    );
    assert!(
        store
            .snapshot()
            .tags
            .tags
            .iter()
            .any(|t| t.name == "v0.2.0")
    );

    let _ = std::fs::remove_dir_all(&repo_dir);
}
