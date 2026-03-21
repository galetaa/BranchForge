use app_host::{
    CommitFlowOutcome, OpenRepoOutcome, run_commit_flow_with_prompt,
    run_repo_open_flow_with_picker, run_status_stage_unstage_smoke,
};

fn unique_temp_dir(prefix: &str) -> std::path::PathBuf {
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0);
    std::env::temp_dir().join(format!("branchforge-{prefix}-{nanos}"))
}

#[test]
fn mvp_end_to_end_smoke_suite() {
    let repo_dir = unique_temp_dir("mvp-smoke");
    assert!(std::fs::create_dir_all(&repo_dir).is_ok());
    assert!(git_service::run_git(&repo_dir, &["init"]).is_ok());
    assert!(git_service::run_git(&repo_dir, &["config", "user.email", "dev@example.com"]).is_ok());
    assert!(git_service::run_git(&repo_dir, &["config", "user.name", "Dev User"]).is_ok());

    let file = "mvp.txt".to_string();
    assert!(std::fs::write(repo_dir.join(&file), "payload\n").is_ok());

    let open_outcome = run_repo_open_flow_with_picker(|| Some(repo_dir.clone()));
    assert!(matches!(open_outcome, OpenRepoOutcome::Opened(_)));
    if let OpenRepoOutcome::Opened(snapshot) = open_outcome {
        assert!(snapshot.repo.is_some());
        assert!(snapshot.status.untracked.iter().any(|p| p == &file));
    }

    let stage_unstage = run_status_stage_unstage_smoke(&repo_dir, vec![file.clone()]);
    assert!(stage_unstage.is_ok());
    if let Ok(snapshot) = stage_unstage {
        assert!(snapshot.status.untracked.iter().any(|p| p == &file));
    }

    assert!(git_service::stage_paths(&repo_dir, std::slice::from_ref(&file)).is_ok());
    let commit_outcome =
        run_commit_flow_with_prompt(&repo_dir, || Some("feat: mvp smoke commit".to_string()));
    assert!(matches!(commit_outcome, CommitFlowOutcome::Committed(_)));
    if let CommitFlowOutcome::Committed(snapshot) = commit_outcome {
        assert!(snapshot.status.staged.is_empty());
        assert!(
            snapshot
                .repo
                .as_ref()
                .and_then(|r| r.head.as_ref())
                .is_some()
        );
    }

    assert!(git_service::run_git(&repo_dir, &["rev-parse", "HEAD"]).is_ok());
    let _ = std::fs::remove_dir_all(&repo_dir);
}
