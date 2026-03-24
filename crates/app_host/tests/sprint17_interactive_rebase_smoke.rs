use job_system::{JobLock, JobRequest, execute_job_op};
use state_store::{RebaseEntryAction, StateStore};

fn unique_temp_dir(label: &str) -> std::path::PathBuf {
    static COUNTER: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0);
    let seq = COUNTER.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    std::env::temp_dir().join(format!("branchforge-sprint17-{label}-{nanos}-{seq}"))
}

fn init_repo() -> std::path::PathBuf {
    let repo_dir = unique_temp_dir("repo");
    assert!(std::fs::create_dir_all(&repo_dir).is_ok());
    assert!(git_service::run_git(&repo_dir, &["init"]).is_ok());
    assert!(git_service::run_git(&repo_dir, &["config", "user.email", "dev@example.com"]).is_ok());
    assert!(git_service::run_git(&repo_dir, &["config", "user.name", "Dev User"]).is_ok());
    repo_dir
}

#[test]
fn rebase_plan_execute_and_history_refresh_work_end_to_end() {
    let repo_dir = init_repo();

    assert!(std::fs::write(repo_dir.join("one.txt"), "one\n").is_ok());
    assert!(git_service::stage_paths(&repo_dir, &["one.txt".to_string()]).is_ok());
    assert!(git_service::commit_create(&repo_dir, "feat: one").is_ok());

    assert!(std::fs::write(repo_dir.join("two.txt"), "two\n").is_ok());
    assert!(git_service::stage_paths(&repo_dir, &["two.txt".to_string()]).is_ok());
    assert!(git_service::commit_create(&repo_dir, "feat: two").is_ok());

    assert!(std::fs::write(repo_dir.join("three.txt"), "three\n").is_ok());
    assert!(git_service::stage_paths(&repo_dir, &["three.txt".to_string()]).is_ok());
    assert!(git_service::commit_create(&repo_dir, "feat: three").is_ok());

    let base = git_service::run_git(&repo_dir, &["rev-list", "--max-parents=0", "HEAD"])
        .ok()
        .and_then(|out| String::from_utf8(out.stdout).ok())
        .unwrap_or_default()
        .trim()
        .to_string();

    let mut store = StateStore::new();
    let plan = execute_job_op(
        &repo_dir,
        &JobRequest {
            op: "rebase.plan.create".to_string(),
            lock: JobLock::RefsWrite,
            paths: vec![base],
            job_id: None,
        },
        &mut store,
    );
    assert!(plan.is_ok());
    assert!(store.snapshot().rebase.plan.is_some());

    if let Some(mut current_plan) = store.snapshot().rebase.plan.clone() {
        if current_plan.entries.len() >= 2 {
            current_plan.entries.swap(0, 1);
        }
        if let Some(last) = current_plan.entries.last_mut() {
            last.action = RebaseEntryAction::Drop;
        }
        store.update_rebase_plan(current_plan);
    }

    let exec = execute_job_op(
        &repo_dir,
        &JobRequest {
            op: "rebase.execute".to_string(),
            lock: JobLock::RefsWrite,
            paths: Vec::new(),
            job_id: None,
        },
        &mut store,
    );
    assert!(exec.is_ok());

    let summaries = git_service::commit_log_page(&repo_dir, 0, 20)
        .ok()
        .unwrap_or_default()
        .into_iter()
        .map(|c| c.summary)
        .collect::<Vec<_>>();
    assert!(summaries.iter().any(|s| s == "feat: one"));
    assert!(summaries.iter().any(|s| s == "feat: two"));
    assert!(!summaries.iter().any(|s| s == "feat: three"));

    let _ = std::fs::remove_dir_all(&repo_dir);
}

#[test]
fn repo_open_restores_rebase_session_from_restart_hook() {
    let repo_dir = init_repo();

    let rebase_merge = git_service::run_git(&repo_dir, &["rev-parse", "--git-path", "rebase-merge"])
        .ok()
        .and_then(|out| String::from_utf8(out.stdout).ok())
        .map(|text| {
            let path = std::path::PathBuf::from(text.trim().to_string());
            if path.is_absolute() {
                path
            } else {
                repo_dir.join(path)
            }
        })
        .unwrap_or_else(|| repo_dir.join(".git").join("rebase-merge"));
    assert!(std::fs::create_dir_all(&rebase_merge).is_ok());
    assert!(std::fs::write(rebase_merge.join("msgnum"), "2\n").is_ok());
    assert!(std::fs::write(rebase_merge.join("end"), "5\n").is_ok());

    let mut store = StateStore::new();
    let opened = execute_job_op(
        &repo_dir,
        &JobRequest {
            op: "repo.open".to_string(),
            lock: JobLock::Read,
            paths: Vec::new(),
            job_id: None,
        },
        &mut store,
    );
    assert!(opened.is_ok());

    let session = store.snapshot().rebase.session.clone();
    assert!(session.is_some());
    if let Some(session) = session {
        assert!(session.active);
        assert_eq!(session.current_step, Some(2));
        assert_eq!(session.total_steps, Some(5));
    }

    let _ = std::fs::remove_dir_all(&repo_dir);
}


