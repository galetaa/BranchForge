use state_store::{CommitDetails, JournalStatus, StateStore};

#[test]
fn sprint23_beta_hardening_smoke() {
    let mut store = StateStore::new();

    for idx in 0..400 {
        store.update_commit_details(CommitDetails {
            oid: format!("oid-{idx}"),
            author: "Dev".to_string(),
            time: "now".to_string(),
            message: format!("message-{idx}"),
        });
    }
    assert!(store.snapshot().commit_cache.len() <= 256);

    let entry_id = store.append_journal_entry(None, "history.search".to_string(), 10);
    store.finish_journal_entry(entry_id, JournalStatus::Succeeded, 25, None);
    let entry_id = store.append_journal_entry(None, "diff.worktree".to_string(), 30);
    store.finish_journal_entry(
        entry_id,
        JournalStatus::Failed,
        70,
        Some("boom".to_string()),
    );

    let diagnostics = ui_shell::render_diagnostics_panel(&store);
    assert!(diagnostics.contains("Avg duration(ms):"));
    assert!(diagnostics.contains("Actionable blockers: 1"));

    store.update_repo(plugin_api::RepoSnapshot {
        root: "/tmp/demo".to_string(),
        head: Some("main".to_string()),
        conflict_state: None,
    });
    store.update_status(state_store::StatusSnapshot {
        staged: vec!["README.md".to_string()],
        unstaged: Vec::new(),
        untracked: Vec::new(),
    });
    let status = ui_shell::render_status_panel(&store);
    assert!(status.contains("Keyboard hints:"));
}
