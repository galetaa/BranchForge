use std::time::Instant;

use action_engine::{ActionRequest, InvokeError, route_action_invoke, route_action_response};
use plugin_api::{
    ActionContext, ActionSpec, METHOD_EVENT_REPO_OPENED, METHOD_EVENT_STATE_UPDATED, PluginHello,
    RpcMessage, RpcNotification, RpcResponse,
};
use plugin_host::{RuntimeSession, default_registration_payload};
use state_store::{SelectionState, StateEvent, StateStore, StatusSnapshot};

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
    let mut store = StateStore::new();
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
        false,
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
        assert!(rendered.contains("active_view: status.panel"));
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
}
