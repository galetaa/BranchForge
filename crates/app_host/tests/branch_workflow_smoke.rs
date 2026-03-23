use app_host::run_branch_workflow_smoke;

fn unique_temp_dir(label: &str) -> std::path::PathBuf {
    static COUNTER: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0);
    let seq = COUNTER.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    std::env::temp_dir().join(format!("branchforge-branches-{label}-{nanos}-{seq}"))
}

#[test]
fn branch_workflow_smoke() {
    let repo_dir = unique_temp_dir("repo");
    assert!(std::fs::create_dir_all(&repo_dir).is_ok());
    assert!(git_service::run_git(&repo_dir, &["init"]).is_ok());
    assert!(git_service::run_git(&repo_dir, &["config", "user.email", "dev@example.com"]).is_ok());
    assert!(git_service::run_git(&repo_dir, &["config", "user.name", "Dev User"]).is_ok());
    assert!(std::fs::write(repo_dir.join("README.md"), "hello\n").is_ok());
    assert!(git_service::stage_paths(&repo_dir, &["README.md".to_string()]).is_ok());
    assert!(git_service::commit_create(&repo_dir, "base").is_ok());

    let snapshot = run_branch_workflow_smoke(&repo_dir, "feature").expect("branch workflow");
    assert!(
        !snapshot
            .branches
            .branches
            .iter()
            .any(|b| b.name == "feature-renamed")
    );

    let _ = std::fs::remove_dir_all(&repo_dir);
}
