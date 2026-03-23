use app_host::{
    CommitFlowOutcome, OpenRepoOutcome, run_commit_flow_with_prompt, run_history_page_smoke,
    run_history_select_and_diff_smoke, run_repo_open_flow_with_picker,
    run_status_stage_unstage_smoke,
};

fn unique_temp_dir(label: &str) -> std::path::PathBuf {
    static COUNTER: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0);
    let seq = COUNTER.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    std::env::temp_dir().join(format!("branchforge-history-diff-{label}-{nanos}-{seq}"))
}

#[test]
fn history_and_diff_e2e_flow() {
    let repo_dir = unique_temp_dir("repo");
    assert!(std::fs::create_dir_all(&repo_dir).is_ok());
    assert!(git_service::run_git(&repo_dir, &["init"]).is_ok());
    assert!(git_service::run_git(&repo_dir, &["config", "user.email", "dev@example.com"]).is_ok());
    assert!(git_service::run_git(&repo_dir, &["config", "user.name", "Dev User"]).is_ok());

    let file = "history.txt".to_string();
    assert!(std::fs::write(repo_dir.join(&file), "payload\n").is_ok());
    assert!(git_service::stage_paths(&repo_dir, std::slice::from_ref(&file)).is_ok());
    assert!(git_service::commit_create(&repo_dir, "history commit").is_ok());

    let open = run_repo_open_flow_with_picker(|| Some(repo_dir.clone()));
    assert!(matches!(open, OpenRepoOutcome::Opened(_)));

    let (commits, cursor) = run_history_page_smoke(&repo_dir, 0, 10).expect("history page");
    assert!(!commits.is_empty());
    if let Some(cursor) = cursor {
        assert!(cursor.offset > 0);
    }

    let (details, diff) = run_history_select_and_diff_smoke(&repo_dir).expect("history select");
    assert!(details.message.contains("history commit"));
    assert!(diff.content.as_deref().unwrap_or("").contains("commit"));

    assert!(std::fs::write(repo_dir.join(&file), "payload\nmore\n").is_ok());
    let status_flow = run_status_stage_unstage_smoke(&repo_dir, vec![file.clone()]);
    assert!(status_flow.is_ok());
    if let Ok(snapshot) = status_flow {
        assert!(
            snapshot
                .diff
                .content
                .as_deref()
                .unwrap_or("")
                .contains("diff --git")
        );
    }

    assert!(git_service::stage_paths(&repo_dir, std::slice::from_ref(&file)).is_ok());
    let commit_outcome =
        run_commit_flow_with_prompt(&repo_dir, || Some("feat: another commit".to_string()));
    assert!(matches!(commit_outcome, CommitFlowOutcome::Committed(_)));

    let _ = std::fs::remove_dir_all(&repo_dir);
}
