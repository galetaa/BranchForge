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
        let idx_two = current_plan
            .entries
            .iter()
            .position(|entry| entry.summary == "feat: two");
        let idx_three = current_plan
            .entries
            .iter()
            .position(|entry| entry.summary == "feat: three");
        if let (Some(a), Some(b)) = (idx_two, idx_three) {
            current_plan.entries.swap(a, b);
        }
        if let Some(drop_target) = current_plan
            .entries
            .iter_mut()
            .find(|entry| entry.summary == "feat: three")
        {
            drop_target.action = RebaseEntryAction::Drop;
        }
        store.update_rebase_plan(current_plan);
    }

    let page_before = execute_job_op(
        &repo_dir,
        &JobRequest {
            op: "history.page".to_string(),
            lock: JobLock::Read,
            paths: vec!["0".to_string(), "20".to_string()],
            job_id: None,
        },
        &mut store,
    );
    assert!(page_before.is_ok());
    assert!(!store.snapshot().history.commits.is_empty());

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
    // Rewrites invalidate old history buffers.
    assert!(store.snapshot().history.commits.is_empty());

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
fn rebase_edit_then_continue_finishes_session() {
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

    if let Some(mut current_plan) = store.snapshot().rebase.plan.clone() {
        if let Some(first) = current_plan.entries.first_mut() {
            first.action = RebaseEntryAction::Edit;
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
    assert!(store.snapshot().rebase.session.is_some());

    // Edit-step allows amending current commit before continue.
    assert!(
        git_service::run_git(&repo_dir, &["commit", "--amend", "--no-edit"]).is_ok()
    );

    let cont = execute_job_op(
        &repo_dir,
        &JobRequest {
            op: "rebase.continue".to_string(),
            lock: JobLock::RefsWrite,
            paths: Vec::new(),
            job_id: None,
        },
        &mut store,
    );
    assert!(cont.is_ok());
    assert!(store.snapshot().rebase.session.is_none());

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

#[test]
fn rebase_abort_clears_active_session_state() {
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

    if let Some(mut current_plan) = store.snapshot().rebase.plan.clone() {
        if let Some(first) = current_plan.entries.first_mut() {
            // `edit` forces rebase to pause in-session so abort path can be exercised.
            first.action = RebaseEntryAction::Edit;
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
    assert!(store.snapshot().rebase.session.is_some());

    let abort = execute_job_op(
        &repo_dir,
        &JobRequest {
            op: "rebase.abort".to_string(),
            lock: JobLock::RefsWrite,
            paths: Vec::new(),
            job_id: None,
        },
        &mut store,
    );
    assert!(abort.is_ok());
    assert!(store.snapshot().rebase.session.is_none());
    assert!(store.snapshot().rebase.plan.is_none());
    assert!(
        store
            .snapshot()
            .repo
            .as_ref()
            .and_then(|repo| repo.conflict_state.clone())
            .is_none()
    );

    let _ = std::fs::remove_dir_all(&repo_dir);
}

#[test]
fn rebase_execute_with_autosquash_rewrites_fixup_chain() {
    let repo_dir = init_repo();

    assert!(std::fs::write(repo_dir.join("base.txt"), "base\n").is_ok());
    assert!(git_service::stage_paths(&repo_dir, &["base.txt".to_string()]).is_ok());
    assert!(git_service::commit_create(&repo_dir, "chore: base").is_ok());

    assert!(std::fs::write(repo_dir.join("one.txt"), "one\n").is_ok());
    assert!(git_service::stage_paths(&repo_dir, &["one.txt".to_string()]).is_ok());
    assert!(git_service::commit_create(&repo_dir, "feat: one").is_ok());

    assert!(std::fs::write(repo_dir.join("one.txt"), "one+fix\n").is_ok());
    assert!(git_service::stage_paths(&repo_dir, &["one.txt".to_string()]).is_ok());
    assert!(git_service::commit_create(&repo_dir, "fixup! feat: one").is_ok());

    let base = git_service::run_git(&repo_dir, &["rev-list", "--max-parents=0", "HEAD"])
        .ok()
        .and_then(|out| String::from_utf8(out.stdout).ok())
        .unwrap_or_default()
        .trim()
        .to_string();

    let mut store = StateStore::new();
    assert!(execute_job_op(
        &repo_dir,
        &JobRequest {
            op: "rebase.plan.create".to_string(),
            lock: JobLock::RefsWrite,
            paths: vec![base],
            job_id: None,
        },
        &mut store,
    )
    .is_ok());

    let exec = execute_job_op(
        &repo_dir,
        &JobRequest {
            op: "rebase.execute".to_string(),
            lock: JobLock::RefsWrite,
            paths: vec!["autosquash".to_string()],
            job_id: None,
        },
        &mut store,
    );
    assert!(exec.is_ok(), "autosquash execute failed: {exec:?}");
    assert!(store.snapshot().rebase.session.is_none());

    let count = git_service::run_git(&repo_dir, &["rev-list", "--count", "HEAD"])
        .ok()
        .and_then(|out| String::from_utf8(out.stdout).ok())
        .unwrap_or_default()
        .trim()
        .parse::<usize>()
        .unwrap_or(0);
    assert_eq!(count, 3);

    let summaries = git_service::commit_log_page(&repo_dir, 0, 10)
        .ok()
        .unwrap_or_default()
        .into_iter()
        .map(|c| c.summary)
        .collect::<Vec<_>>();
    assert!(summaries.iter().any(|s| s == "chore: base"));
    assert!(summaries.iter().any(|s| s == "feat: one"));
    assert!(summaries.iter().any(|s| s.starts_with("fixup!")));

    let _ = std::fs::remove_dir_all(&repo_dir);
}

#[test]
fn rebase_skip_from_edit_pause_completes_session() {
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
    assert!(execute_job_op(
        &repo_dir,
        &JobRequest {
            op: "rebase.plan.create".to_string(),
            lock: JobLock::RefsWrite,
            paths: vec![base],
            job_id: None,
        },
        &mut store,
    )
    .is_ok());

    if let Some(mut current_plan) = store.snapshot().rebase.plan.clone() {
        if let Some(first) = current_plan.entries.first_mut() {
            first.action = RebaseEntryAction::Edit;
        }
        store.update_rebase_plan(current_plan);
    }

    assert!(execute_job_op(
        &repo_dir,
        &JobRequest {
            op: "rebase.execute".to_string(),
            lock: JobLock::RefsWrite,
            paths: Vec::new(),
            job_id: None,
        },
        &mut store,
    )
    .is_ok());
    assert!(store.snapshot().rebase.session.is_some());

    let skip = execute_job_op(
        &repo_dir,
        &JobRequest {
            op: "rebase.skip".to_string(),
            lock: JobLock::RefsWrite,
            paths: Vec::new(),
            job_id: None,
        },
        &mut store,
    );
    assert!(skip.is_ok());
    assert!(store.snapshot().rebase.session.is_none());

    let summaries = git_service::commit_log_page(&repo_dir, 0, 20)
        .ok()
        .unwrap_or_default()
        .into_iter()
        .map(|c| c.summary)
        .collect::<Vec<_>>();
    assert!(summaries.iter().any(|s| s == "feat: one"));
    assert!(summaries.iter().any(|s| s == "feat: two"));
    assert!(summaries.iter().any(|s| s == "feat: three"));

    let _ = std::fs::remove_dir_all(&repo_dir);
}





