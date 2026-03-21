use app_host::{
    run_palette_invoke_smoke, run_palette_when_smoke, run_selection_event_smoke,
    run_state_notification_smoke, run_ui_state_smoke, run_window_layout_smoke,
};

#[test]
fn ui_state_smoke_contains_status_groups_and_commit() {
    let rendered = run_ui_state_smoke();

    assert!(rendered.contains("Status Panel"));
    assert!(rendered.contains("staged: src/lib.rs"));
    assert!(rendered.contains("unstaged: README.md"));
    assert!(rendered.contains("untracked: notes.txt"));
    assert!(rendered.contains("[Commit] enabled -> commit.create"));
}

#[test]
fn palette_smoke_executes_repo_open() {
    let result = run_palette_invoke_smoke("open");
    assert!(result.is_ok());
    if let Ok(action_id) = result {
        assert_eq!(action_id, "repo.open");
    }
}

#[test]
fn state_notifications_smoke_contains_required_events() {
    let methods = run_state_notification_smoke();
    assert!(methods.iter().any(|m| m == "event.repo.opened"));
    assert!(methods.iter().any(|m| m == "event.state.updated"));
}

#[test]
fn window_layout_smoke_contains_slots_and_active_view() {
    let rendered = run_window_layout_smoke();
    assert!(rendered.contains("[left-slot]"));
    assert!(rendered.contains("[service]"));
    assert!(rendered.contains("active_view: status.panel"));
}

#[test]
fn palette_when_smoke_hides_and_shows_actions() {
    let closed = run_palette_when_smoke(false);
    assert!(
        closed
            .iter()
            .any(|(id, enabled)| id == "commit.create" && !*enabled)
    );

    let opened = run_palette_when_smoke(true);
    assert!(
        opened
            .iter()
            .any(|(id, enabled)| id == "commit.create" && *enabled)
    );
}

#[test]
fn selection_event_smoke_updates_selected_paths() {
    let selection = run_selection_event_smoke(vec!["README.md".to_string()]);
    assert_eq!(selection.selected_paths, vec!["README.md".to_string()]);
}
