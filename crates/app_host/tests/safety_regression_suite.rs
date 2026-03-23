use std::path::PathBuf;

use action_engine::{ActionRequest, InvokeError, route_action_invoke};
use app_host::errors::translate_job_error;
use git_service::GitServiceError;
use job_system::{JobExecutionError, JobLock, JobRequest, execute_job_op};
use plugin_api::{ActionContext, DangerLevel, PluginHello};
use plugin_host::{RuntimeSession, branches_registration_payload};
use state_store::{JournalStatus, StateStore};

fn unique_temp_dir() -> PathBuf {
    static COUNTER: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0);
    let seq = COUNTER.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    std::env::temp_dir().join(format!("branchforge-safety-{nanos}-{seq}"))
}

fn init_repo() -> PathBuf {
    let repo_dir = unique_temp_dir();
    std::fs::create_dir_all(&repo_dir).expect("create dir");
    git_service::run_git(&repo_dir, &["init"]).expect("git init");
    git_service::run_git(&repo_dir, &["config", "user.email", "dev@example.com"])
        .expect("set email");
    git_service::run_git(&repo_dir, &["config", "user.name", "Dev User"]).expect("set name");
    repo_dir
}

#[test]
fn high_risk_action_requires_confirmation() {
    let mut session = RuntimeSession::new("branches");
    let hello = PluginHello {
        plugin_id: "branches".to_string(),
        version: "0.1".to_string(),
    };
    session.handle_hello(&hello).expect("hello");
    session
        .handle_register(&branches_registration_payload())
        .expect("register");

    let action = ActionRequest {
        action: "branch.delete".to_string(),
        confirmed: false,
    };
    let routed = route_action_invoke(
        &mut session,
        &action,
        ActionContext {
            selection_files: Vec::new(),
        },
        std::time::Instant::now(),
    );

    assert!(matches!(
        routed,
        Err(InvokeError::ConfirmationRequired { action_id, danger })
            if action_id == "branch.delete" && danger == DangerLevel::High
    ));
}

#[test]
fn confirmed_high_risk_action_is_routed() {
    let mut session = RuntimeSession::new("branches");
    let hello = PluginHello {
        plugin_id: "branches".to_string(),
        version: "0.1".to_string(),
    };
    session.handle_hello(&hello).expect("hello");
    session
        .handle_register(&branches_registration_payload())
        .expect("register");

    let action = ActionRequest {
        action: "branch.delete".to_string(),
        confirmed: true,
    };
    let routed = route_action_invoke(
        &mut session,
        &action,
        ActionContext {
            selection_files: Vec::new(),
        },
        std::time::Instant::now(),
    );

    assert!(routed.is_ok());
    assert_eq!(session.pending_count(), 1);
}

#[test]
fn journal_records_commit_operations() {
    let repo_dir = init_repo();
    std::fs::write(repo_dir.join("entry.txt"), "data\n").expect("write file");

    let mut store = StateStore::new();
    execute_job_op(
        &repo_dir,
        &JobRequest {
            op: "index.stage_paths".to_string(),
            lock: JobLock::IndexWrite,
            paths: vec!["entry.txt".to_string()],
            job_id: None,
        },
        &mut store,
    )
    .expect("stage");

    execute_job_op(
        &repo_dir,
        &JobRequest {
            op: "commit.create".to_string(),
            lock: JobLock::RefsWrite,
            paths: vec!["journal commit".to_string()],
            job_id: None,
        },
        &mut store,
    )
    .expect("commit");

    let entries = &store.snapshot().journal.entries;
    assert!(entries.iter().any(|entry| entry.op == "index.stage_paths"));
    assert!(entries.iter().any(|entry| {
        entry.op == "commit.create"
            && entry.job_id.is_none()
            && matches!(entry.status, JournalStatus::Succeeded)
            && entry.finished_at_ms.is_some()
    }));

    let _ = std::fs::remove_dir_all(&repo_dir);
}

#[test]
fn checkout_errors_include_recovery_hint() {
    let err = JobExecutionError::Git(GitServiceError::GitError {
        exit_code: 1,
        stderr: "error: Your local changes to the following files would be overwritten by checkout"
            .to_string(),
    });

    let translated = translate_job_error(&err);
    assert_eq!(translated.title, "Working tree not clean");
    assert_eq!(
        translated.message,
        "Commit or stash changes before checkout."
    );
    assert!(translated.detail.is_some());
}
