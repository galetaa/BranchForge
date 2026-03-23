use std::time::Instant;

use action_engine::{ActionRequest, InvokeError, route_action_invoke, route_action_response};
use job_system::{JobLock, JobRequest, execute_job_op};
use plugin_api::{
    ActionContext, ActionSpec, METHOD_EVENT_REPO_OPENED, METHOD_EVENT_STATE_UPDATED, PluginHello,
    RpcMessage, RpcNotification, RpcResponse,
};
use plugin_host::{
    RuntimeSession, default_registration_payload, repo_manager_registration_payload,
};
use state_store::{
    CommitDetails, CommitSummary, DiffState, HistoryCursor, SelectionState, StateEvent, StateStore,
    StatusSnapshot,
};

pub mod errors;
pub mod recent_repos;

use errors::{UserFacingError, translate_job_error};

pub fn run_action_roundtrip(action_id: &str) -> Result<String, InvokeError> {
    let mut session = RuntimeSession::new("status");
    let hello = PluginHello {
        plugin_id: "status".to_string(),
        version: "0.1".to_string(),
    };

    let hello_result = session.handle_hello(&hello);
    if let Err(err) = hello_result {
        return Err(InvokeError::Session(err));
    }

    let register_result = session.handle_register(&default_registration_payload());
    if let Err(err) = register_result {
        return Err(InvokeError::Session(err));
    }

    let invoke = route_action_invoke(
        &mut session,
        &ActionRequest {
            action: action_id.to_string(),
            confirmed: false,
        },
        ActionContext {
            selection_files: Vec::new(),
        },
        Instant::now(),
    )?;

    let inbound = RpcMessage::Response(RpcResponse::ok(invoke.id, serde_json::json!({"ok": true})));
    let resolved = route_action_response(&mut session, &inbound)?;
    Ok(resolved.unwrap_or_else(String::new))
}

pub fn run_ui_state_smoke() -> String {
    let mut store = StateStore::new();
    store.update_status(StatusSnapshot {
        staged: vec!["src/lib.rs".to_string()],
        unstaged: vec!["README.md".to_string()],
        untracked: vec!["notes.txt".to_string()],
    });
    store.update_selection(SelectionState {
        selected_paths: vec!["README.md".to_string()],
        selected_commit_oid: None,
        selected_branch: None,
    });

    ui_shell::render_status_panel(&store)
}

pub fn run_palette_invoke_smoke(filter: &str) -> Result<String, InvokeError> {
    let mut session = RuntimeSession::new("status");
    let hello = PluginHello {
        plugin_id: "status".to_string(),
        version: "0.1".to_string(),
    };
    if let Err(err) = session.handle_hello(&hello) {
        return Err(InvokeError::Session(err));
    }
    if let Err(err) = session.handle_register(&default_registration_payload()) {
        return Err(InvokeError::Session(err));
    }

    let palette = ui_shell::palette::build_palette(&session.list_actions(), filter, false);
    let first_enabled = palette.into_iter().find(|item| item.enabled);
    let item = match first_enabled {
        Some(item) => item,
        None => return Ok(String::new()),
    };

    let invoke = route_action_invoke(
        &mut session,
        &ActionRequest {
            action: item.action_id,
            confirmed: false,
        },
        ActionContext {
            selection_files: Vec::new(),
        },
        Instant::now(),
    )?;

    let inbound = RpcMessage::Response(RpcResponse::ok(invoke.id, serde_json::json!({"ok": true})));
    let resolved = route_action_response(&mut session, &inbound)?;
    Ok(resolved.unwrap_or_else(String::new))
}

pub fn run_state_notification_smoke() -> Vec<String> {
    let mut store = StateStore::new();
    let sub_id = store.subscribe();

    let mut session = RuntimeSession::new("status");
    session.subscribe(METHOD_EVENT_REPO_OPENED);
    session.subscribe(METHOD_EVENT_STATE_UPDATED);

    store.update_repo(plugin_api::RepoSnapshot {
        root: ".".to_string(),
        head: Some("main".to_string()),
    });
    store.update_status(StatusSnapshot {
        staged: vec!["src/lib.rs".to_string()],
        unstaged: Vec::new(),
        untracked: Vec::new(),
    });

    let events = store.poll_events(sub_id);
    for event in events {
        let notification = match event {
            StateEvent::RepoOpened => RpcNotification::new(
                METHOD_EVENT_REPO_OPENED,
                serde_json::json!({"repo_open": true}),
            ),
            StateEvent::Updated { version } => RpcNotification::new(
                METHOD_EVENT_STATE_UPDATED,
                serde_json::json!({"version": version}),
            ),
        };
        let _ = session.deliver_notification(notification);
    }

    session
        .drain_notifications()
        .into_iter()
        .map(|n| n.method)
        .collect()
}

pub fn run_window_layout_smoke() -> String {
    let store = StateStore::new();

    let palette_items = ui_shell::palette::build_palette(
        &[plugin_api::ActionSpec {
            action_id: "repo.open".to_string(),
            title: "Open Repository".to_string(),
            when: Some("always".to_string()),
            params_schema: None,
            danger: None,
        }],
        "",
        false,
    );

    ui_shell::render_window(&store, &palette_items)
}

pub fn run_window_after_open_smoke() -> String {
    let mut store = StateStore::new();
    store.update_repo(plugin_api::RepoSnapshot {
        root: "/tmp/demo".to_string(),
        head: Some("main".to_string()),
    });
    store.update_status(StatusSnapshot {
        staged: vec!["src/lib.rs".to_string()],
        unstaged: vec!["README.md".to_string()],
        untracked: Vec::new(),
    });

    let palette_items = ui_shell::palette::build_palette(
        &[plugin_api::ActionSpec {
            action_id: "repo.open".to_string(),
            title: "Open Repository".to_string(),
            when: Some("always".to_string()),
            params_schema: None,
            danger: None,
        }],
        "",
        true,
    );

    ui_shell::render_window(&store, &palette_items)
}

pub fn run_palette_when_smoke(repo_open: bool) -> Vec<(String, bool)> {
    let actions = vec![
        ActionSpec {
            action_id: "repo.open".to_string(),
            title: "Open Repository".to_string(),
            when: Some("always".to_string()),
            params_schema: None,
            danger: None,
        },
        ActionSpec {
            action_id: "commit.create".to_string(),
            title: "Commit".to_string(),
            when: Some("repo.is_open".to_string()),
            params_schema: None,
            danger: None,
        },
    ];

    ui_shell::palette::build_palette(&actions, "", repo_open)
        .into_iter()
        .map(|item| (item.action_id, item.enabled))
        .collect()
}

pub fn run_selection_event_smoke(selected_paths: Vec<String>) -> SelectionState {
    let mut store = StateStore::new();
    store.update_selection(SelectionState {
        selected_paths,
        selected_commit_oid: None,
        selected_branch: None,
    });
    store.snapshot().selection.clone()
}

pub fn run_git_jobs_smoke(
    repo_dir: &std::path::Path,
) -> Result<state_store::StoreSnapshot, String> {
    let mut store = StateStore::new();

    let open = execute_job_op(
        repo_dir,
        &JobRequest {
            op: "repo.open".to_string(),
            lock: JobLock::Read,
            paths: Vec::new(),
            job_id: None,
        },
        &mut store,
    )
    .map_err(|err| format!("repo.open failed: {err:?}"))?;

    if !open.success {
        return Err("repo.open returned unsuccessful result".to_string());
    }

    Ok(store.snapshot().clone())
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum OpenRepoOutcome {
    Opened(Box<state_store::StoreSnapshot>),
    Cancelled,
    Failed(UserFacingError),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CommitFlowOutcome {
    Committed(Box<state_store::StoreSnapshot>),
    Cancelled,
    ValidationError(UserFacingError),
    Failed(UserFacingError),
}

pub fn run_repo_open_flow_with_picker<F>(mut pick_folder: F) -> OpenRepoOutcome
where
    F: FnMut() -> Option<std::path::PathBuf>,
{
    let mut session = RuntimeSession::new("repo_manager");
    let hello = PluginHello {
        plugin_id: "repo_manager".to_string(),
        version: "0.1".to_string(),
    };
    if session.handle_hello(&hello).is_err() {
        return OpenRepoOutcome::Failed(UserFacingError::new(
            "Plugin error",
            "Repo manager handshake failed.",
            None,
        ));
    }
    if session
        .handle_register(&repo_manager_registration_payload())
        .is_err()
    {
        return OpenRepoOutcome::Failed(UserFacingError::new(
            "Plugin error",
            "Repo manager registration failed.",
            None,
        ));
    }

    let selected = pick_folder();
    let repo_dir = match selected {
        Some(path) => path,
        None => return OpenRepoOutcome::Cancelled,
    };

    let mut store = StateStore::new();
    let result = execute_job_op(
        &repo_dir,
        &JobRequest {
            op: "repo.open".to_string(),
            lock: JobLock::Read,
            paths: Vec::new(),
            job_id: None,
        },
        &mut store,
    );

    match result {
        Ok(_) => {
            let _ = recent_repos::persist_recent_repo(&repo_dir);
            OpenRepoOutcome::Opened(Box::new(store.snapshot().clone()))
        }
        Err(err) => OpenRepoOutcome::Failed(translate_job_error(&err)),
    }
}

pub fn run_status_stage_unstage_smoke(
    repo_dir: &std::path::Path,
    selected_files: Vec<String>,
) -> Result<state_store::StoreSnapshot, String> {
    let mut store = StateStore::new();

    execute_job_op(
        repo_dir,
        &JobRequest {
            op: "repo.open".to_string(),
            lock: JobLock::Read,
            paths: Vec::new(),
            job_id: None,
        },
        &mut store,
    )
    .map_err(|e| format!("repo.open failed: {e:?}"))?;

    store.update_selection(SelectionState {
        selected_paths: selected_files.clone(),
        selected_commit_oid: None,
        selected_branch: None,
    });

    let _ = execute_job_op(
        repo_dir,
        &JobRequest {
            op: "diff.worktree".to_string(),
            lock: JobLock::Read,
            paths: selected_files.clone(),
            job_id: None,
        },
        &mut store,
    );

    execute_job_op(
        repo_dir,
        &JobRequest {
            op: "index.stage_paths".to_string(),
            lock: JobLock::IndexWrite,
            paths: selected_files.clone(),
            job_id: None,
        },
        &mut store,
    )
    .map_err(|e| format!("stage failed: {e:?}"))?;

    execute_job_op(
        repo_dir,
        &JobRequest {
            op: "index.unstage_paths".to_string(),
            lock: JobLock::IndexWrite,
            paths: selected_files,
            job_id: None,
        },
        &mut store,
    )
    .map_err(|e| format!("unstage failed: {e:?}"))?;

    Ok(store.snapshot().clone())
}

pub fn run_history_page_smoke(
    repo_dir: &std::path::Path,
    offset: usize,
    limit: usize,
) -> Result<(Vec<CommitSummary>, Option<HistoryCursor>), String> {
    let mut store = StateStore::new();
    store.set_active_view(Some("history.panel".to_string()));
    execute_job_op(
        repo_dir,
        &JobRequest {
            op: "history.page".to_string(),
            lock: JobLock::Read,
            paths: vec![offset.to_string(), limit.to_string()],
            job_id: None,
        },
        &mut store,
    )
    .map_err(|e| format!("history.page failed: {e:?}"))?;

    Ok((
        store.snapshot().history.commits.clone(),
        store.snapshot().history.next_cursor.clone(),
    ))
}

pub fn run_history_select_and_diff_smoke(
    repo_dir: &std::path::Path,
) -> Result<(CommitDetails, DiffState), String> {
    let mut store = StateStore::new();
    execute_job_op(
        repo_dir,
        &JobRequest {
            op: "history.page".to_string(),
            lock: JobLock::Read,
            paths: vec!["0".to_string(), "5".to_string()],
            job_id: None,
        },
        &mut store,
    )
    .map_err(|e| format!("history.page failed: {e:?}"))?;

    let commit = store
        .snapshot()
        .history
        .commits
        .first()
        .cloned()
        .ok_or_else(|| "history page empty".to_string())?;

    store.update_selected_commit(Some(commit.oid.clone()));

    execute_job_op(
        repo_dir,
        &JobRequest {
            op: "history.details".to_string(),
            lock: JobLock::Read,
            paths: vec![commit.oid.clone()],
            job_id: None,
        },
        &mut store,
    )
    .map_err(|e| format!("history.details failed: {e:?}"))?;

    execute_job_op(
        repo_dir,
        &JobRequest {
            op: "diff.commit".to_string(),
            lock: JobLock::Read,
            paths: vec![commit.oid.clone()],
            job_id: None,
        },
        &mut store,
    )
    .map_err(|e| format!("diff.commit failed: {e:?}"))?;

    let details = store
        .commit_details(&commit.oid)
        .cloned()
        .ok_or_else(|| "commit details missing".to_string())?;

    Ok((details, store.snapshot().diff.clone()))
}

pub fn run_branch_workflow_smoke(
    repo_dir: &std::path::Path,
    branch_name: &str,
) -> Result<state_store::StoreSnapshot, String> {
    let mut store = StateStore::new();
    execute_job_op(
        repo_dir,
        &JobRequest {
            op: "repo.open".to_string(),
            lock: JobLock::Read,
            paths: Vec::new(),
            job_id: None,
        },
        &mut store,
    )
    .map_err(|e| format!("repo.open failed: {e:?}"))?;

    let base_branch = store
        .snapshot()
        .repo
        .as_ref()
        .and_then(|repo| repo.head.clone())
        .unwrap_or_else(|| "main".to_string());

    execute_job_op(
        repo_dir,
        &JobRequest {
            op: "branch.create".to_string(),
            lock: JobLock::RefsWrite,
            paths: vec![branch_name.to_string()],
            job_id: None,
        },
        &mut store,
    )
    .map_err(|e| format!("branch.create failed: {e:?}"))?;

    store.update_selected_branch(Some(branch_name.to_string()));
    store.set_active_view(Some("branches.panel".to_string()));

    execute_job_op(
        repo_dir,
        &JobRequest {
            op: "branch.checkout".to_string(),
            lock: JobLock::RefsWrite,
            paths: vec![branch_name.to_string()],
            job_id: None,
        },
        &mut store,
    )
    .map_err(|e| format!("branch.checkout failed: {e:?}"))?;

    let renamed = format!("{branch_name}-renamed");
    execute_job_op(
        repo_dir,
        &JobRequest {
            op: "branch.rename".to_string(),
            lock: JobLock::RefsWrite,
            paths: vec![branch_name.to_string(), renamed.clone()],
            job_id: None,
        },
        &mut store,
    )
    .map_err(|e| format!("branch.rename failed: {e:?}"))?;

    execute_job_op(
        repo_dir,
        &JobRequest {
            op: "branch.checkout".to_string(),
            lock: JobLock::RefsWrite,
            paths: vec![base_branch.clone()],
            job_id: None,
        },
        &mut store,
    )
    .map_err(|e| format!("branch.checkout base failed: {e:?}"))?;

    execute_job_op(
        repo_dir,
        &JobRequest {
            op: "branch.delete".to_string(),
            lock: JobLock::RefsWrite,
            paths: vec![renamed],
            job_id: None,
        },
        &mut store,
    )
    .map_err(|e| format!("branch.delete failed: {e:?}"))?;

    Ok(store.snapshot().clone())
}

pub fn run_commit_flow_with_prompt<F>(
    repo_dir: &std::path::Path,
    mut prompt_message: F,
) -> CommitFlowOutcome
where
    F: FnMut() -> Option<String>,
{
    let mut store = StateStore::new();

    if execute_job_op(
        repo_dir,
        &JobRequest {
            op: "repo.open".to_string(),
            lock: JobLock::Read,
            paths: Vec::new(),
            job_id: None,
        },
        &mut store,
    )
    .is_err()
    {
        return CommitFlowOutcome::Failed(UserFacingError::new(
            "Not a Git repository",
            "Select a folder that contains a Git repository.",
            None,
        ));
    }

    if store.snapshot().status.staged.is_empty() {
        return CommitFlowOutcome::ValidationError(UserFacingError::new(
            "Nothing to commit",
            "No staged changes to commit.",
            None,
        ));
    }

    let message = match prompt_message() {
        Some(message) => message,
        None => return CommitFlowOutcome::Cancelled,
    };

    store.update_commit_message(message.clone(), None);
    if let Some(error) = validate_commit_message(&message) {
        store.update_commit_message(message, Some(error.clone()));
        return CommitFlowOutcome::ValidationError(UserFacingError::new(
            "Invalid input",
            &error,
            None,
        ));
    }

    let result = execute_job_op(
        repo_dir,
        &JobRequest {
            op: "commit.create".to_string(),
            lock: JobLock::RefsWrite,
            paths: vec![message],
            job_id: None,
        },
        &mut store,
    );

    match result {
        Ok(_) => CommitFlowOutcome::Committed(Box::new(store.snapshot().clone())),
        Err(err) => CommitFlowOutcome::Failed(translate_job_error(&err)),
    }
}

pub fn run_commit_amend_smoke(
    repo_dir: &std::path::Path,
    message: Option<String>,
) -> Result<state_store::StoreSnapshot, String> {
    let mut store = StateStore::new();
    execute_job_op(
        repo_dir,
        &JobRequest {
            op: "commit.amend".to_string(),
            lock: JobLock::RefsWrite,
            paths: message.into_iter().collect(),
            job_id: None,
        },
        &mut store,
    )
    .map_err(|e| format!("commit.amend failed: {e:?}"))?;

    Ok(store.snapshot().clone())
}

pub fn run_history_search_smoke(
    repo_dir: &std::path::Path,
    author: Option<String>,
    text: Option<String>,
) -> Result<Vec<CommitSummary>, String> {
    let mut store = StateStore::new();
    let mut paths = vec!["0".to_string(), "20".to_string()];
    paths.push(author.clone().unwrap_or_default());
    paths.push(text.clone().unwrap_or_default());

    execute_job_op(
        repo_dir,
        &JobRequest {
            op: "history.page".to_string(),
            lock: JobLock::Read,
            paths,
            job_id: None,
        },
        &mut store,
    )
    .map_err(|e| format!("history.search failed: {e:?}"))?;

    Ok(store.snapshot().history.commits.clone())
}

fn validate_commit_message(message: &str) -> Option<String> {
    if message.trim().is_empty() {
        Some("Commit message cannot be empty.".to_string())
    } else {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn roundtrip_returns_action_id() {
        let result = run_action_roundtrip("repo.open");
        assert!(result.is_ok());
        if let Ok(action_id) = result {
            assert_eq!(action_id, "repo.open");
        }
    }

    #[test]
    fn ui_state_smoke_renders_status_panel() {
        let rendered = run_ui_state_smoke();
        assert!(rendered.contains("Status Panel"));
        assert!(rendered.contains("staged: src/lib.rs"));
        assert!(rendered.contains("[Commit] enabled"));
    }

    #[test]
    fn palette_smoke_invokes_action() {
        let result = run_palette_invoke_smoke("open");
        assert!(result.is_ok());
        if let Ok(action_id) = result {
            assert_eq!(action_id, "repo.open");
        }
    }

    #[test]
    fn state_notifications_smoke_delivers_repo_and_update_events() {
        let methods = run_state_notification_smoke();
        assert!(methods.iter().any(|m| m == METHOD_EVENT_REPO_OPENED));
        assert!(methods.iter().any(|m| m == METHOD_EVENT_STATE_UPDATED));
    }

    #[test]
    fn window_layout_smoke_renders_slots() {
        let rendered = run_window_layout_smoke();
        assert!(rendered.contains("[left-slot]"));
        assert!(rendered.contains("[service]"));
        assert!(rendered.contains("active_view: empty.state"));
        assert!(rendered.contains("No repository opened"));
    }

    #[test]
    fn window_layout_switches_after_repo_open() {
        let rendered = run_window_after_open_smoke();
        assert!(rendered.contains("active_view: status.panel"));
        assert!(rendered.contains("Status Panel"));
    }

    #[test]
    fn palette_when_smoke_hides_and_shows_by_when() {
        let closed_repo = run_palette_when_smoke(false);
        assert!(
            closed_repo
                .iter()
                .any(|(id, enabled)| id == "repo.open" && *enabled)
        );
        assert!(
            closed_repo
                .iter()
                .any(|(id, enabled)| id == "commit.create" && !*enabled)
        );

        let open_repo = run_palette_when_smoke(true);
        assert!(
            open_repo
                .iter()
                .any(|(id, enabled)| id == "commit.create" && *enabled)
        );
    }

    #[test]
    fn selection_event_smoke_updates_selection_state() {
        let selection =
            run_selection_event_smoke(vec!["README.md".to_string(), "src/lib.rs".to_string()]);
        assert_eq!(selection.selected_paths.len(), 2);
        assert!(selection.selected_paths.iter().any(|p| p == "README.md"));
    }

    #[test]
    fn git_jobs_smoke_updates_repo_and_status() {
        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_nanos())
            .unwrap_or(0);
        let repo_dir = std::env::temp_dir().join(format!("branchforge-app-host-git-smoke-{nanos}"));

        assert!(std::fs::create_dir_all(&repo_dir).is_ok());
        assert!(git_service::run_git(&repo_dir, &["init"]).is_ok());
        assert!(std::fs::write(repo_dir.join("README.md"), "hello\n").is_ok());

        let result = run_git_jobs_smoke(&repo_dir);
        assert!(result.is_ok());
        if let Ok(snapshot) = result {
            assert!(snapshot.repo.is_some());
            assert!(snapshot.status.untracked.iter().any(|p| p == "README.md"));
        }

        let _ = std::fs::remove_dir_all(&repo_dir);
    }

    #[test]
    fn repo_open_flow_handles_cancel_path() {
        let outcome = run_repo_open_flow_with_picker(|| None);
        assert!(matches!(outcome, OpenRepoOutcome::Cancelled));
    }

    #[test]
    fn repo_open_flow_reports_invalid_repo_error() {
        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_nanos())
            .unwrap_or(0);
        let dir = std::env::temp_dir().join(format!("branchforge-invalid-repo-{nanos}"));
        assert!(std::fs::create_dir_all(&dir).is_ok());

        let outcome = run_repo_open_flow_with_picker(|| Some(dir.clone()));
        assert!(matches!(
            outcome,
            OpenRepoOutcome::Failed(UserFacingError { title, message, detail: Some(_) })
                if title == "Not a Git repository"
                && message == "Select a folder that contains a Git repository."
        ));

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn status_stage_unstage_smoke_updates_status_without_restart() {
        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_nanos())
            .unwrap_or(0);
        let repo_dir = std::env::temp_dir().join(format!("branchforge-status-smoke-{nanos}"));
        assert!(std::fs::create_dir_all(&repo_dir).is_ok());
        assert!(git_service::run_git(&repo_dir, &["init"]).is_ok());

        let file = "tracked.txt".to_string();
        assert!(std::fs::write(repo_dir.join(&file), "data\n").is_ok());

        let result = run_status_stage_unstage_smoke(&repo_dir, vec![file.clone()]);
        assert!(result.is_ok());
        if let Ok(snapshot) = result {
            assert!(snapshot.status.untracked.iter().any(|p| p == &file));
        }

        let _ = std::fs::remove_dir_all(&repo_dir);
    }

    #[test]
    fn commit_flow_with_prompt_commits_staged_changes() {
        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_nanos())
            .unwrap_or(0);
        let repo_dir = std::env::temp_dir().join(format!("branchforge-commit-smoke-{nanos}"));
        assert!(std::fs::create_dir_all(&repo_dir).is_ok());
        assert!(git_service::run_git(&repo_dir, &["init"]).is_ok());
        assert!(
            git_service::run_git(&repo_dir, &["config", "user.email", "dev@example.com"]).is_ok()
        );
        assert!(git_service::run_git(&repo_dir, &["config", "user.name", "Dev User"]).is_ok());

        let file = "commit.txt".to_string();
        assert!(std::fs::write(repo_dir.join(&file), "payload\n").is_ok());
        assert!(git_service::stage_paths(&repo_dir, std::slice::from_ref(&file)).is_ok());

        let outcome =
            run_commit_flow_with_prompt(&repo_dir, || Some("feat: add commit flow".to_string()));
        assert!(matches!(outcome, CommitFlowOutcome::Committed(_)));
        if let CommitFlowOutcome::Committed(snapshot) = outcome {
            assert!(snapshot.status.staged.is_empty());
        }

        let _ = std::fs::remove_dir_all(&repo_dir);
    }

    #[test]
    fn commit_flow_with_prompt_supports_cancel() {
        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_nanos())
            .unwrap_or(0);
        let repo_dir = std::env::temp_dir().join(format!("branchforge-commit-cancel-{nanos}"));
        assert!(std::fs::create_dir_all(&repo_dir).is_ok());
        assert!(git_service::run_git(&repo_dir, &["init"]).is_ok());
        assert!(
            git_service::run_git(&repo_dir, &["config", "user.email", "dev@example.com"]).is_ok()
        );
        assert!(git_service::run_git(&repo_dir, &["config", "user.name", "Dev User"]).is_ok());

        let file = "cancel.txt".to_string();
        assert!(std::fs::write(repo_dir.join(&file), "payload\n").is_ok());
        assert!(git_service::stage_paths(&repo_dir, std::slice::from_ref(&file)).is_ok());

        let outcome = run_commit_flow_with_prompt(&repo_dir, || None);
        assert!(matches!(outcome, CommitFlowOutcome::Cancelled));

        let _ = std::fs::remove_dir_all(&repo_dir);
    }

    #[test]
    fn commit_flow_with_prompt_rejects_empty_message() {
        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_nanos())
            .unwrap_or(0);
        let repo_dir = std::env::temp_dir().join(format!("branchforge-commit-empty-{nanos}"));
        assert!(std::fs::create_dir_all(&repo_dir).is_ok());
        assert!(git_service::run_git(&repo_dir, &["init"]).is_ok());
        assert!(
            git_service::run_git(&repo_dir, &["config", "user.email", "dev@example.com"]).is_ok()
        );
        assert!(git_service::run_git(&repo_dir, &["config", "user.name", "Dev User"]).is_ok());

        let file = "empty.txt".to_string();
        assert!(std::fs::write(repo_dir.join(&file), "payload\n").is_ok());
        assert!(git_service::stage_paths(&repo_dir, std::slice::from_ref(&file)).is_ok());

        let outcome = run_commit_flow_with_prompt(&repo_dir, || Some("   ".to_string()));
        assert!(matches!(
            outcome,
            CommitFlowOutcome::ValidationError(UserFacingError { title, message, .. })
                if title == "Invalid input" && message == "Commit message cannot be empty."
        ));

        let _ = std::fs::remove_dir_all(&repo_dir);
    }

    #[test]
    fn commit_flow_rejects_without_staged_changes() {
        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_nanos())
            .unwrap_or(0);
        let repo_dir = std::env::temp_dir().join(format!("branchforge-commit-empty-stage-{nanos}"));
        assert!(std::fs::create_dir_all(&repo_dir).is_ok());
        assert!(git_service::run_git(&repo_dir, &["init"]).is_ok());
        assert!(
            git_service::run_git(&repo_dir, &["config", "user.email", "dev@example.com"]).is_ok()
        );
        assert!(git_service::run_git(&repo_dir, &["config", "user.name", "Dev User"]).is_ok());
        assert!(std::fs::write(repo_dir.join("README.md"), "hello\n").is_ok());

        let outcome =
            run_commit_flow_with_prompt(&repo_dir, || Some("feat: try commit".to_string()));
        assert!(matches!(
            outcome,
            CommitFlowOutcome::ValidationError(UserFacingError { title, message, .. })
                if title == "Nothing to commit" && message == "No staged changes to commit."
        ));

        let _ = std::fs::remove_dir_all(&repo_dir);
    }
}
