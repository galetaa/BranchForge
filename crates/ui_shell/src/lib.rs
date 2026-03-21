use state_store::StateStore;

pub mod layout;
pub mod palette;
pub mod viewmodel;

pub fn render_root(store: &StateStore) -> String {
    match store.repo() {
        Some(repo) => format!("Repo: {}", repo.root),
        None => "Repo: <not opened>".to_string(),
    }
}

pub fn render_status_panel(store: &StateStore) -> String {
    let panel = viewmodel::build_status_panel(store.snapshot());
    viewmodel::render(&panel, store.snapshot())
}

pub fn render_window(store: &StateStore, palette_items: &[palette::PaletteItem]) -> String {
    let status = render_status_panel(store);
    let layout = layout::build_layout(&status, palette_items, Some("status.panel".to_string()));
    layout::render_layout(&layout)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn renders_empty_state() {
        let store = StateStore::new();
        assert_eq!(render_root(&store), "Repo: <not opened>");
    }

    #[test]
    fn renders_status_panel_with_lists() {
        let mut store = StateStore::new();
        store.update_status(state_store::StatusSnapshot {
            staged: vec!["src/lib.rs".to_string()],
            unstaged: vec!["README.md".to_string()],
            untracked: vec!["notes.txt".to_string()],
        });

        let rendered = render_status_panel(&store);
        assert!(rendered.contains("staged: src/lib.rs"));
        assert!(rendered.contains("unstaged: README.md"));
        assert!(rendered.contains("untracked: notes.txt"));
    }

    #[test]
    fn builds_palette_with_when_rules() {
        let items = palette::build_palette(
            &[plugin_api::ActionSpec {
                action_id: "commit.create".to_string(),
                title: "Commit".to_string(),
                when: Some("repo.is_open".to_string()),
                params_schema: None,
            }],
            "",
            false,
        );

        assert_eq!(items.len(), 1);
        assert!(!items[0].enabled);
    }

    #[test]
    fn renders_window_with_slots() {
        let mut store = StateStore::new();
        store.update_status(state_store::StatusSnapshot {
            staged: vec!["src/lib.rs".to_string()],
            unstaged: Vec::new(),
            untracked: Vec::new(),
        });

        let palette_items = palette::build_palette(
            &[plugin_api::ActionSpec {
                action_id: "repo.open".to_string(),
                title: "Open Repository".to_string(),
                when: Some("always".to_string()),
                params_schema: None,
            }],
            "",
            false,
        );

        let rendered = render_window(&store, &palette_items);
        assert!(rendered.contains("[left-slot]"));
        assert!(rendered.contains("[service]"));
    }
}
