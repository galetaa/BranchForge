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

fn render_plugin_warnings(store: &StateStore) -> Option<String> {
    let warnings = store
        .snapshot()
        .plugins
        .iter()
        .filter_map(|status| match &status.health {
            state_store::PluginHealth::Unavailable { message } => Some(format!(
                "plugin {} unavailable: {}",
                status.plugin_id, message
            )),
            state_store::PluginHealth::Ready => None,
        })
        .collect::<Vec<_>>();

    if warnings.is_empty() {
        None
    } else {
        Some(format!("Plugin warnings:\n{}", warnings.join("\n")))
    }
}

fn view_owner_plugin(active_view: &str) -> Option<&'static str> {
    match active_view {
        "status.panel" => Some("status"),
        "history.panel" => Some("history"),
        "branches.panel" => Some("branches"),
        _ => None,
    }
}

fn degraded_view_message(store: &StateStore, active_view: &str) -> Option<String> {
    let owner = view_owner_plugin(active_view)?;
    let unavailable = store.snapshot().plugins.iter().find(|status| {
        status.plugin_id == owner
            && matches!(status.health, state_store::PluginHealth::Unavailable { .. })
    })?;

    let reason = match &unavailable.health {
        state_store::PluginHealth::Unavailable { message } => message.as_str(),
        state_store::PluginHealth::Ready => return None,
    };

    Some(format!(
        "[Degraded View]\nview: {active_view}\nplugin: {owner}\nreason: {reason}\naction: plugin will recover automatically after successful restart"
    ))
}

pub fn render_status_panel(store: &StateStore) -> String {
    let panel = viewmodel::build_status_panel(store.snapshot());
    viewmodel::render(&panel, store.snapshot())
}

pub fn render_history_panel(store: &StateStore) -> String {
    let panel = viewmodel::build_history_panel(store.snapshot());
    viewmodel::render(&panel, store.snapshot())
}

pub fn render_branches_panel(store: &StateStore) -> String {
    let panel = viewmodel::build_branches_panel(store.snapshot());
    viewmodel::render(&panel, store.snapshot())
}

pub fn render_diff_panel(store: &StateStore) -> String {
    let diff = &store.snapshot().diff;
    if diff.loading {
        return "Diff: loading...".to_string();
    }
    if let Some(err) = &diff.error {
        return format!("Diff error: {err}");
    }
    let mut out = diff
        .content
        .as_ref()
        .map(|content| format!("Diff:\n{content}"))
        .unwrap_or_else(|| "Diff: <empty>".to_string());

    if let Some(descriptor) = diff.descriptor.as_ref() {
        out.push_str(&format!(
            "\nDescriptor: bytes={}, chunk_size={}, loaded_chunks={}, truncated={}\n",
            descriptor.total_bytes,
            descriptor.chunk_size,
            descriptor.loaded_chunks,
            descriptor.truncated
        ));
    }

    if !diff.chunks.is_empty() {
        let preview = diff
            .chunks
            .iter()
            .map(|chunk| format!("#{}({}b)", chunk.index, chunk.content.len()))
            .collect::<Vec<_>>()
            .join(", ");
        out.push_str(&format!("Chunks: {preview}\n"));
    }

    if let Some(state_store::DiffSource::Compare { base, head }) = diff.source.as_ref() {
        out.push_str(&format!("\nCompare: {base} -> {head}\n"));
    }

    if !diff.hunks.is_empty() {
        out.push_str("\nHunks:\n");
        for hunk in &diff.hunks {
            out.push_str(&format!(
                "  {}[{}] {}\n",
                hunk.file_path, hunk.hunk_index, hunk.header
            ));
            let action = match diff.source {
                Some(state_store::DiffSource::Worktree { .. }) => Some("index.stage_hunk"),
                Some(state_store::DiffSource::Index { .. }) => Some("index.unstage_hunk"),
                _ => None,
            };
            if let Some(action_id) = action {
                out.push_str(&format!(
                    "  [Hunk Action] enabled -> {} {} {}\n",
                    action_id, hunk.file_path, hunk.hunk_index
                ));
            }
        }
    }

    out
}

pub fn render_empty_state() -> String {
    "No repository opened. Use `Open Repository` from command palette.".to_string()
}

pub fn render_window(store: &StateStore, palette_items: &[palette::PaletteItem]) -> String {
    let palette_items = palette::apply_plugin_health(palette_items, &store.snapshot().plugins);

    let active_view = if let Some(active) = store.snapshot().active_view.as_ref() {
        Some(active.clone())
    } else if store.repo().is_some() {
        Some("status.panel".to_string())
    } else {
        Some("empty.state".to_string())
    };

    let left_slot = if let Some(active) = active_view.as_deref() {
        if let Some(degraded) = degraded_view_message(store, active) {
            degraded
        } else {
            match active {
                "history.panel" => render_history_panel(store),
                "branches.panel" => render_branches_panel(store),
                "status.panel" => render_status_panel(store),
                _ => {
                    if store.repo().is_some() {
                        render_status_panel(store)
                    } else {
                        render_empty_state()
                    }
                }
            }
        }
    } else {
        render_empty_state()
    };

    let left_slot = if let Some(warnings) = render_plugin_warnings(store) {
        format!("{warnings}\n{left_slot}")
    } else {
        left_slot
    };

    let diff_panel = if store.snapshot().diff.source.is_some() {
        Some(render_diff_panel(store))
    } else {
        None
    };
    let layout = layout::build_layout(&left_slot, &palette_items, diff_panel, active_view);
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
                danger: None,
            }],
            "",
            false,
        );

        assert_eq!(items.len(), 1);
        assert!(!items[0].enabled);
    }

    #[test]
    fn renders_window_with_slots() {
        let store = StateStore::new();

        let palette_items = palette::build_palette(
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

        let rendered = render_window(&store, &palette_items);
        assert!(rendered.contains("[left-slot]"));
        assert!(rendered.contains("[service]"));
        assert!(rendered.contains("No repository opened"));
        assert!(rendered.contains("active_view: empty.state"));
    }

    #[test]
    fn switches_to_status_panel_after_repo_open() {
        let mut store = StateStore::new();
        store.update_repo(plugin_api::RepoSnapshot {
            root: "/tmp/demo".to_string(),
            head: Some("main".to_string()),
            conflict_state: None,
        });
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
                danger: None,
            }],
            "",
            true,
        );

        let rendered = render_window(&store, &palette_items);
        assert!(rendered.contains("active_view: status.panel"));
        assert!(rendered.contains("Status Panel"));
    }

    #[test]
    fn renders_history_panel_when_active_view_set() {
        let mut store = StateStore::new();
        store.set_active_view(Some("history.panel".to_string()));
        store.update_history_page(
            vec![state_store::CommitSummary {
                oid: "abc123".to_string(),
                author: "Dev".to_string(),
                time: "now".to_string(),
                summary: "Initial commit".to_string(),
            }],
            None,
            false,
            None,
            None,
        );
        store.update_selected_commit(Some("abc123".to_string()));

        let palette_items = palette::build_palette(&[], "", true);
        let rendered = render_window(&store, &palette_items);
        assert!(rendered.contains("History Panel"));
        assert!(rendered.contains("commits: *abc123 Initial commit"));
    }

    #[test]
    fn renders_branches_panel_when_active_view_set() {
        let mut store = StateStore::new();
        store.set_active_view(Some("branches.panel".to_string()));
        store.update_branches(vec![
            state_store::BranchInfo {
                name: "main".to_string(),
                is_current: true,
                upstream: None,
            },
            state_store::BranchInfo {
                name: "feature".to_string(),
                is_current: false,
                upstream: None,
            },
        ]);

        let palette_items = palette::build_palette(&[], "", true);
        let rendered = render_window(&store, &palette_items);
        assert!(rendered.contains("Branches Panel"));
        assert!(rendered.contains("branches: *main,  feature"));
    }

    #[test]
    fn renders_plugin_warning_block() {
        let mut store = StateStore::new();
        store.update_plugin_status(
            "status",
            state_store::PluginHealth::Unavailable {
                message: "crashed".to_string(),
            },
        );

        let palette_items = palette::build_palette(&[], "", false);
        let rendered = render_window(&store, &palette_items);
        assert!(rendered.contains("Plugin warnings:"));
        assert!(rendered.contains("plugin status unavailable: crashed"));
    }

    #[test]
    fn renders_degraded_container_for_unavailable_active_plugin_view() {
        let mut store = StateStore::new();
        store.update_repo(plugin_api::RepoSnapshot {
            root: "/tmp/demo".to_string(),
            head: Some("main".to_string()),
            conflict_state: None,
        });
        store.set_active_view(Some("status.panel".to_string()));
        store.update_plugin_status(
            "status",
            state_store::PluginHealth::Unavailable {
                message: "restarting".to_string(),
            },
        );

        let rendered = render_window(&store, &palette::build_palette(&[], "", true));
        assert!(rendered.contains("[Degraded View]"));
        assert!(rendered.contains("view: status.panel"));
        assert!(rendered.contains("plugin: status"));
        assert!(rendered.contains("reason: restarting"));
    }

    #[test]
    fn falls_back_to_normal_panel_when_plugin_ready() {
        let mut store = StateStore::new();
        store.update_repo(plugin_api::RepoSnapshot {
            root: "/tmp/demo".to_string(),
            head: Some("main".to_string()),
            conflict_state: None,
        });
        store.set_active_view(Some("status.panel".to_string()));
        store.update_status(state_store::StatusSnapshot {
            staged: vec!["src/lib.rs".to_string()],
            unstaged: Vec::new(),
            untracked: Vec::new(),
        });
        store.update_plugin_status("status", state_store::PluginHealth::Ready);

        let rendered = render_window(&store, &palette::build_palette(&[], "", true));
        assert!(rendered.contains("Status Panel"));
        assert!(!rendered.contains("[Degraded View]"));
    }

    #[test]
    fn disables_palette_action_with_unavailable_plugin_reason() {
        let mut store = StateStore::new();
        store.update_repo(plugin_api::RepoSnapshot {
            root: "/tmp/demo".to_string(),
            head: Some("main".to_string()),
            conflict_state: None,
        });
        store.update_plugin_status(
            "status",
            state_store::PluginHealth::Unavailable {
                message: "restarting".to_string(),
            },
        );

        let palette_items = palette::build_palette(
            &[plugin_api::ActionSpec {
                action_id: "commit.create".to_string(),
                title: "Commit".to_string(),
                when: Some("repo.is_open".to_string()),
                params_schema: None,
                danger: None,
            }],
            "",
            true,
        );
        let rendered = render_window(&store, &palette_items);
        assert!(rendered.contains("Commit (off: plugin status unavailable: restarting)"));
    }

    #[test]
    fn keeps_palette_action_enabled_when_plugin_ready() {
        let mut store = StateStore::new();
        store.update_repo(plugin_api::RepoSnapshot {
            root: "/tmp/demo".to_string(),
            head: Some("main".to_string()),
            conflict_state: None,
        });
        store.update_plugin_status("status", state_store::PluginHealth::Ready);

        let palette_items = palette::build_palette(
            &[plugin_api::ActionSpec {
                action_id: "commit.create".to_string(),
                title: "Commit".to_string(),
                when: Some("repo.is_open".to_string()),
                params_schema: None,
                danger: None,
            }],
            "",
            true,
        );
        let rendered = render_window(&store, &palette_items);
        assert!(rendered.contains("Commit (on)"));
    }
}
