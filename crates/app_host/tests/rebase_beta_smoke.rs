use app_host::run_rebase_beta_smoke;

fn unique_temp_dir(label: &str) -> std::path::PathBuf {
    static COUNTER: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0);
    let seq = COUNTER.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    std::env::temp_dir().join(format!("branchforge-rebase-beta-{label}-{nanos}-{seq}"))
}

fn init_repo() -> std::path::PathBuf {
    let repo_dir = unique_temp_dir("repo");
    assert!(std::fs::create_dir_all(&repo_dir).is_ok());
    assert!(git_service::run_git(&repo_dir, &["init"]).is_ok());
    repo_dir
}

#[test]
fn rebase_interactive_returns_preview_when_available() {
    let repo_dir = init_repo();
    let result = run_rebase_beta_smoke(&repo_dir);
    assert!(result.is_ok());
    if let Ok(preview) = result {
        assert_eq!(preview.preflight.action_id, "rebase.interactive");
        assert_eq!(preview.preview.title, "Interactive Rebase");
    }

    let _ = std::fs::remove_dir_all(&repo_dir);
}
