use app_host::{
    CommitFlowOutcome, OpenRepoOutcome, run_commit_flow_with_prompt,
    run_repo_open_flow_with_picker, run_status_stage_unstage_smoke,
};
use plugin_api;
use plugin_host::{
    PluginAvailability, PluginProcess, PluginProcessConfig, PluginSupervisor, RestartPolicy,
};
use state_store::{PluginHealth, StateStore, StatusSnapshot};
use ui_shell;

fn unique_temp_dir(label: &str) -> std::path::PathBuf {
    static COUNTER: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0);
    let seq = COUNTER.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    std::env::temp_dir().join(format!("branchforge-post-mvp-{label}-{nanos}-{seq}"))
}

#[test]
fn post_mvp_smoke_baseline() {
    let repo_dir = unique_temp_dir("repo");
    assert!(std::fs::create_dir_all(&repo_dir).is_ok());
    assert!(git_service::run_git(&repo_dir, &["init"]).is_ok());
    assert!(git_service::run_git(&repo_dir, &["config", "user.email", "dev@example.com"]).is_ok());
    assert!(git_service::run_git(&repo_dir, &["config", "user.name", "Dev User"]).is_ok());

    let file = "baseline.txt".to_string();
    assert!(std::fs::write(repo_dir.join(&file), "payload\n").is_ok());

    let open_outcome = run_repo_open_flow_with_picker(|| Some(repo_dir.clone()));
    assert!(matches!(open_outcome, OpenRepoOutcome::Opened(_)));

    let stage_unstage = run_status_stage_unstage_smoke(&repo_dir, vec![file.clone()]);
    assert!(stage_unstage.is_ok());

    assert!(git_service::stage_paths(&repo_dir, std::slice::from_ref(&file)).is_ok());
    let commit_outcome =
        run_commit_flow_with_prompt(&repo_dir, || Some("feat: baseline".to_string()));
    assert!(matches!(commit_outcome, CommitFlowOutcome::Committed(_)));

    let config = PluginProcessConfig {
        plugin_id: "status".to_string(),
        program: "sh".to_string(),
        args: vec!["-c".to_string(), "exit 1".to_string()],
        restart_policy: RestartPolicy::Never,
    };
    let process = PluginProcess::spawn(config).expect("spawn");
    let mut supervisor = PluginSupervisor::new(process);
    std::thread::sleep(std::time::Duration::from_millis(20));
    supervisor.poll().expect("poll");

    let mut store = StateStore::new();
    store.update_repo(plugin_api::RepoSnapshot {
        root: repo_dir.display().to_string(),
        head: Some("main".to_string()),
    });
    store.update_status(StatusSnapshot {
        staged: Vec::new(),
        unstaged: Vec::new(),
        untracked: Vec::new(),
    });
    if let PluginAvailability::Unavailable { reason } = supervisor.availability() {
        store.update_plugin_status(
            supervisor.plugin_id(),
            PluginHealth::Unavailable {
                message: reason.clone(),
            },
        );
    }

    let palette_items = ui_shell::palette::build_palette(
        &[plugin_api::ActionSpec {
            action_id: "repo.open".to_string(),
            title: "Open Repository".to_string(),
            when: Some("always".to_string()),
            params_schema: None,
        }],
        "",
        true,
    );
    let rendered = ui_shell::render_window(&store, &palette_items);
    assert!(rendered.contains("Plugin warnings:"));
    assert!(rendered.contains("plugin status unavailable"));

    let _ = std::fs::remove_dir_all(&repo_dir);
}
