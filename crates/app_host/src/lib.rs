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
use state_store::{SelectionState, StateEvent, StateStore, StatusSnapshot};

pub mod recent_repos;

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
        },
        ActionSpec {
            action_id: "commit.create".to_string(),
            title: "Commit".to_string(),
            when: Some("repo.is_open".to_string()),
            params_schema: None,
        },
    ];

    ui_shell::palette::build_palette(&actions, "", repo_open)
        .into_iter()
        .map(|item| (item.action_id, item.enabled))
        .collect()
}

pub fn run_selection_event_smoke(selected_paths: Vec<String>) -> SelectionState {
    let mut store = StateStore::new();
    store.update_selection(SelectionState { selected_paths });
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
    Opened(state_store::StoreSnapshot),
    Cancelled,
    Failed(String),
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
        return OpenRepoOutcome::Failed("repo manager handshake failed".to_string());
    }
    if session
        .handle_register(&repo_manager_registration_payload())
        .is_err()
    {
        return OpenRepoOutcome::Failed("repo manager registration failed".to_string());
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
        },
        &mut store,
    );

    match result {
        Ok(_) => {
            let _ = recent_repos::persist_recent_repo(&repo_dir);
            OpenRepoOutcome::Opened(store.snapshot().clone())
        }
        Err(_) => OpenRepoOutcome::Failed("Selected folder is not a git repository.".to_string()),
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
            OpenRepoOutcome::Failed(message) if message == "Selected folder is not a git repository."
        ));

        let _ = std::fs::remove_dir_all(&dir);
    }
}
