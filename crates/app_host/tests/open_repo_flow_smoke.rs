use app_host::{OpenRepoOutcome, run_repo_open_flow_with_picker, run_status_stage_unstage_smoke};

fn unique_temp_dir(prefix: &str) -> std::path::PathBuf {
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0);
    std::env::temp_dir().join(format!("branchforge-{prefix}-{nanos}"))
}

#[test]
fn open_repo_flow_smoke_success() {
    let repo_dir = unique_temp_dir("open-repo");
    assert!(std::fs::create_dir_all(&repo_dir).is_ok());
    assert!(git_service::run_git(&repo_dir, &["init"]).is_ok());
    assert!(std::fs::write(repo_dir.join("README.md"), "hello\n").is_ok());

    let outcome = run_repo_open_flow_with_picker(|| Some(repo_dir.clone()));
    assert!(matches!(outcome, OpenRepoOutcome::Opened(_)));

    if let OpenRepoOutcome::Opened(snapshot) = outcome {
        assert!(snapshot.repo.is_some());
        assert!(snapshot.status.untracked.iter().any(|p| p == "README.md"));
    }

    let _ = std::fs::remove_dir_all(&repo_dir);
}

#[test]
fn open_repo_flow_smoke_cancel() {
    let outcome = run_repo_open_flow_with_picker(|| None);
    assert!(matches!(outcome, OpenRepoOutcome::Cancelled));
}

#[test]
fn status_stage_unstage_flow_smoke() {
    let repo_dir = unique_temp_dir("status-stage-unstage");
    assert!(std::fs::create_dir_all(&repo_dir).is_ok());
    assert!(git_service::run_git(&repo_dir, &["init"]).is_ok());
    let file = "file.txt".to_string();
    assert!(std::fs::write(repo_dir.join(&file), "payload\n").is_ok());

    let result = run_status_stage_unstage_smoke(&repo_dir, vec![file.clone()]);
    assert!(result.is_ok());
    if let Ok(snapshot) = result {
        assert!(snapshot.status.untracked.iter().any(|p| p == &file));
    }

    let _ = std::fs::remove_dir_all(&repo_dir);
}
