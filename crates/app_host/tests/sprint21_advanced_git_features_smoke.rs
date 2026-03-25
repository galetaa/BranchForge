use job_system::{JobLock, JobRequest, execute_job_op};
use state_store::StateStore;

fn unique_temp_dir() -> std::path::PathBuf {
    static COUNTER: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0);
    let seq = COUNTER.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    std::env::temp_dir().join(format!("branchforge-sprint21-smoke-{nanos}-{seq}"))
}

#[test]
fn sprint21_worktree_submodule_and_capabilities_flow() {
    let root = unique_temp_dir();
    let repo_dir = root.join("main");
    let sub_repo = root.join("sub");
    let worktree_dir = root.join("wt");

    assert!(std::fs::create_dir_all(&repo_dir).is_ok());
    assert!(git_service::run_git(&repo_dir, &["init"]).is_ok());
    assert!(git_service::run_git(&repo_dir, &["config", "user.email", "dev@example.com"]).is_ok());
    assert!(git_service::run_git(&repo_dir, &["config", "user.name", "Dev User"]).is_ok());

    assert!(std::fs::write(repo_dir.join("README.md"), "base\n").is_ok());
    assert!(git_service::stage_paths(&repo_dir, &["README.md".to_string()]).is_ok());
    assert!(git_service::commit_create(&repo_dir, "base").is_ok());

    // Mark LFS capability baseline.
    assert!(
        std::fs::write(
            repo_dir.join(".gitattributes"),
            "*.bin filter=lfs diff=lfs merge=lfs -text\n"
        )
        .is_ok()
    );

    let mut store = StateStore::new();
    assert!(
        execute_job_op(
            &repo_dir,
            &JobRequest {
                op: "diagnostics.repo_capabilities".to_string(),
                lock: JobLock::Read,
                paths: Vec::new(),
                job_id: None,
            },
            &mut store,
        )
        .is_ok()
    );
    let caps = store.snapshot().diff.content.clone().unwrap_or_default();
    assert!(caps.contains("lfs_detected: true"));

    assert!(
        execute_job_op(
            &repo_dir,
            &JobRequest {
                op: "worktree.create".to_string(),
                lock: JobLock::RefsWrite,
                paths: vec![
                    worktree_dir.to_string_lossy().to_string(),
                    "wt-branch".to_string()
                ],
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
                op: "worktree.list".to_string(),
                lock: JobLock::Read,
                paths: Vec::new(),
                job_id: None,
            },
            &mut store,
        )
        .is_ok()
    );
    let worktrees = store.snapshot().diff.content.clone().unwrap_or_default();
    assert!(worktrees.contains("wt"));

    assert!(
        execute_job_op(
            &repo_dir,
            &JobRequest {
                op: "worktree.open".to_string(),
                lock: JobLock::Read,
                paths: vec![worktree_dir.to_string_lossy().to_string()],
                job_id: None,
            },
            &mut store,
        )
        .is_ok()
    );
    let opened_root = store
        .snapshot()
        .repo
        .as_ref()
        .map(|repo| repo.root.clone())
        .unwrap_or_default();
    assert!(opened_root.contains("wt"));

    assert!(
        execute_job_op(
            &repo_dir,
            &JobRequest {
                op: "worktree.remove".to_string(),
                lock: JobLock::RefsWrite,
                paths: vec![
                    worktree_dir.to_string_lossy().to_string(),
                    "force".to_string()
                ],
                job_id: None,
            },
            &mut store,
        )
        .is_ok()
    );

    // Prepare local submodule.
    assert!(std::fs::create_dir_all(&sub_repo).is_ok());
    assert!(git_service::run_git(&sub_repo, &["init"]).is_ok());
    assert!(git_service::run_git(&sub_repo, &["config", "user.email", "dev@example.com"]).is_ok());
    assert!(git_service::run_git(&sub_repo, &["config", "user.name", "Dev User"]).is_ok());
    assert!(std::fs::write(sub_repo.join("SUB.md"), "sub\n").is_ok());
    assert!(git_service::stage_paths(&sub_repo, &["SUB.md".to_string()]).is_ok());
    assert!(git_service::commit_create(&sub_repo, "sub init").is_ok());

    assert!(
        git_service::run_git(
            &repo_dir,
            &[
                "-c",
                "protocol.file.allow=always",
                "submodule",
                "add",
                sub_repo.to_string_lossy().as_ref(),
                "deps/sub"
            ]
        )
        .is_ok()
    );

    assert!(
        execute_job_op(
            &repo_dir,
            &JobRequest {
                op: "submodule.list".to_string(),
                lock: JobLock::Read,
                paths: Vec::new(),
                job_id: None,
            },
            &mut store,
        )
        .is_ok()
    );
    let submodules = store.snapshot().diff.content.clone().unwrap_or_default();
    assert!(submodules.contains("deps/sub"));

    assert!(
        execute_job_op(
            &repo_dir,
            &JobRequest {
                op: "submodule.open".to_string(),
                lock: JobLock::Read,
                paths: vec!["deps/sub".to_string()],
                job_id: None,
            },
            &mut store,
        )
        .is_ok()
    );

    let _ = std::fs::remove_dir_all(&root);
}
