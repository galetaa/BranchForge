use app_host::errors::translate_job_error;
use app_host::run_branch_workflow_smoke;
use job_system::{JobExecutionError, JobLock, JobRequest, execute_job_op};
use state_store::StateStore;

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

#[test]
fn dirty_worktree_blocks_branch_checkout_with_clear_error() {
    let repo_dir = unique_temp_dir("dirty-checkout");
    assert!(std::fs::create_dir_all(&repo_dir).is_ok());
    assert!(git_service::run_git(&repo_dir, &["init"]).is_ok());
    assert!(git_service::run_git(&repo_dir, &["config", "user.email", "dev@example.com"]).is_ok());
    assert!(git_service::run_git(&repo_dir, &["config", "user.name", "Dev User"]).is_ok());

    assert!(std::fs::write(repo_dir.join("README.md"), "base\n").is_ok());
    assert!(git_service::stage_paths(&repo_dir, &["README.md".to_string()]).is_ok());
    assert!(git_service::commit_create(&repo_dir, "base").is_ok());

    let mut store = StateStore::new();
    assert!(
        execute_job_op(
            &repo_dir,
            &JobRequest {
                op: "repo.open".to_string(),
                lock: JobLock::Read,
                paths: Vec::new(),
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
                op: "branch.create".to_string(),
                lock: JobLock::RefsWrite,
                paths: vec!["feature".to_string()],
                job_id: None,
            },
            &mut store,
        )
        .is_ok()
    );

    assert!(std::fs::write(repo_dir.join("README.md"), "dirty\n").is_ok());
    let checkout = execute_job_op(
        &repo_dir,
        &JobRequest {
            op: "branch.checkout".to_string(),
            lock: JobLock::RefsWrite,
            paths: vec!["feature".to_string()],
            job_id: None,
        },
        &mut store,
    );

    assert!(matches!(
        checkout,
        Err(JobExecutionError::InvalidInput { .. })
    ));
    let translated = translate_job_error(&checkout.expect_err("expected invalid input"));
    assert_eq!(translated.title, "Invalid input");
    assert!(translated.message.contains("uncommitted changes"));

    let _ = std::fs::remove_dir_all(&repo_dir);
}

#[test]
fn current_branch_is_visible_in_all_core_panels_after_checkout() {
    let repo_dir = unique_temp_dir("branch-signal");
    assert!(std::fs::create_dir_all(&repo_dir).is_ok());
    assert!(git_service::run_git(&repo_dir, &["init"]).is_ok());
    assert!(git_service::run_git(&repo_dir, &["config", "user.email", "dev@example.com"]).is_ok());
    assert!(git_service::run_git(&repo_dir, &["config", "user.name", "Dev User"]).is_ok());

    assert!(std::fs::write(repo_dir.join("README.md"), "base\n").is_ok());
    assert!(git_service::stage_paths(&repo_dir, &["README.md".to_string()]).is_ok());
    assert!(git_service::commit_create(&repo_dir, "base").is_ok());

    let mut store = StateStore::new();
    assert!(
        execute_job_op(
            &repo_dir,
            &JobRequest {
                op: "repo.open".to_string(),
                lock: JobLock::Read,
                paths: Vec::new(),
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
                op: "branch.create".to_string(),
                lock: JobLock::RefsWrite,
                paths: vec!["feature/signal".to_string()],
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
                op: "branch.checkout".to_string(),
                lock: JobLock::RefsWrite,
                paths: vec!["feature/signal".to_string()],
                job_id: None,
            },
            &mut store,
        )
        .is_ok()
    );

    let status = ui_shell::render_status_panel(&store);
    let history = ui_shell::render_history_panel(&store);
    let branches = ui_shell::render_branches_panel(&store);
    assert!(status.contains("Current branch: feature/signal"));
    assert!(history.contains("Current branch: feature/signal"));
    assert!(branches.contains("Current branch: feature/signal"));

    let _ = std::fs::remove_dir_all(&repo_dir);
}
