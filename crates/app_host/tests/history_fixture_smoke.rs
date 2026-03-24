use app_host::run_history_page_smoke;

fn unique_temp_dir(label: &str) -> std::path::PathBuf {
    static COUNTER: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0);
    let seq = COUNTER.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    std::env::temp_dir().join(format!("branchforge-history-fixture-{label}-{nanos}-{seq}"))
}

fn init_repo_with_commits() -> std::path::PathBuf {
    let repo_dir = unique_temp_dir("repo");
    assert!(std::fs::create_dir_all(&repo_dir).is_ok());
    assert!(git_service::run_git(&repo_dir, &["init"]).is_ok());
    assert!(git_service::run_git(&repo_dir, &["config", "user.email", "dev@example.com"]).is_ok());
    assert!(git_service::run_git(&repo_dir, &["config", "user.name", "Dev User"]).is_ok());

    for idx in 1..=3 {
        let file = repo_dir.join(format!("commit-{idx}.txt"));
        assert!(std::fs::write(&file, format!("commit {idx}\n")).is_ok());
        assert!(git_service::stage_paths(&repo_dir, &[format!("commit-{idx}.txt")]).is_ok());
        assert!(git_service::commit_create(&repo_dir, &format!("commit {idx}")).is_ok());
    }

    repo_dir
}

#[test]
fn history_fixture_pages_commits() {
    let repo_dir = init_repo_with_commits();
    let (first_page, cursor) = run_history_page_smoke(&repo_dir, 0, 2).expect("page 1");
    assert_eq!(first_page.len(), 2);
    assert!(cursor.is_some());

    let (second_page, _) = run_history_page_smoke(&repo_dir, 2, 2).expect("page 2");
    assert!(!second_page.is_empty());

    let _ = std::fs::remove_dir_all(&repo_dir);
}
