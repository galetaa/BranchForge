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
        "tags.panel" => Some("tags"),
        "compare.panel" => Some("compare"),
        "diagnostics.panel" => Some("diagnostics"),
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

pub fn render_tags_panel(store: &StateStore) -> String {
    let panel = viewmodel::build_tags_panel(store.snapshot());
    viewmodel::render(&panel, store.snapshot())
}

pub fn render_compare_panel(store: &StateStore) -> String {
    let panel = viewmodel::build_compare_panel(store.snapshot());
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
            let actions: &[&str] = match diff.source {
                Some(state_store::DiffSource::Worktree { .. }) => {
                    &["index.stage_hunk", "file.discard_hunk"]
                }
                Some(state_store::DiffSource::Index { .. }) => &["index.unstage_hunk"],
                _ => &[],
            };
            for action_id in actions {
                out.push_str(&format!(
                    "  [Hunk Action] enabled -> {} {} {}\n",
                    action_id, hunk.file_path, hunk.hunk_index
                ));
            }
        }
    }

    out
}

pub fn render_diagnostics_panel(store: &StateStore) -> String {
    let host_version = env!("CARGO_PKG_VERSION");
    let protocol_version = plugin_api::HOST_PLUGIN_PROTOCOL_VERSION;
    let entries = &store.snapshot().journal.entries;
    let started = entries
        .iter()
        .filter(|entry| matches!(entry.status, state_store::JournalStatus::Started))
        .count();
    let succeeded = entries
        .iter()
        .filter(|entry| matches!(entry.status, state_store::JournalStatus::Succeeded))
        .count();
    let failed = entries
        .iter()
        .filter(|entry| matches!(entry.status, state_store::JournalStatus::Failed))
        .count();
    let last_error = entries
        .iter()
        .rev()
        .find(|entry| matches!(entry.status, state_store::JournalStatus::Failed))
        .and_then(|entry| entry.error.clone())
        .unwrap_or_else(|| "<none>".to_string());
    let durations = entries
        .iter()
        .filter_map(|entry| match (entry.started_at_ms, entry.finished_at_ms) {
            (start, Some(end)) if end >= start => Some((entry.op.as_str(), end - start)),
            _ => None,
        })
        .collect::<Vec<_>>();
    let avg_duration_ms = if durations.is_empty() {
        0
    } else {
        durations.iter().map(|(_, ms)| *ms).sum::<u64>() / durations.len() as u64
    };
    let slowest = durations
        .iter()
        .max_by_key(|(_, ms)| *ms)
        .map(|(op, ms)| format!("{op} ({ms}ms)"))
        .unwrap_or_else(|| "<none>".to_string());
    let actionable_blockers = entries
        .iter()
        .filter(|entry| matches!(entry.status, state_store::JournalStatus::Failed))
        .count();
    let rebase_plan = store.snapshot().rebase.plan.as_ref().map(|plan| {
        format!(
            "base={} commits={} autosquash={}",
            plan.base_ref, plan.affected_commit_count, plan.autosquash_aware
        )
    });
    let rebase_session = store.snapshot().rebase.session.as_ref().map(|session| {
        format!(
            "active={} step={}/{}",
            session.active,
            session
                .current_step
                .map(|value| value.to_string())
                .unwrap_or_else(|| "?".to_string()),
            session
                .total_steps
                .map(|value| value.to_string())
                .unwrap_or_else(|| "?".to_string())
        )
    });
    let runtime_plugin_health = if store.snapshot().plugins.is_empty() {
        "<none>".to_string()
    } else {
        store
            .snapshot()
            .plugins
            .iter()
            .map(|status| match &status.health {
                state_store::PluginHealth::Ready => format!("{}=ready", status.plugin_id),
                state_store::PluginHealth::Unavailable { message } => {
                    format!("{}=unavailable({message})", status.plugin_id)
                }
            })
            .collect::<Vec<_>>()
            .join(", ")
    };
    let selected_plugin = store
        .snapshot()
        .selection
        .selected_plugin_id
        .as_deref()
        .unwrap_or("<none>");
    let installed_plugins = if store.snapshot().installed_plugins.is_empty() {
        format!("Selected plugin: {selected_plugin}\nInstalled plugins: 0\n<empty>")
    } else {
        let mut lines = vec![
            format!("Selected plugin: {selected_plugin}"),
            format!(
                "Installed plugins: {}",
                store.snapshot().installed_plugins.len()
            ),
        ];
        for plugin in &store.snapshot().installed_plugins {
            let marker = if store.snapshot().selection.selected_plugin_id.as_deref()
                == Some(plugin.plugin_id.as_str())
            {
                "*"
            } else {
                "-"
            };
            let enabled = if plugin.enabled {
                "enabled"
            } else {
                "disabled"
            };
            let permissions = if plugin.permissions.is_empty() {
                "<none>".to_string()
            } else {
                plugin.permissions.join(", ")
            };
            lines.push(format!(
                "{marker} {} v{} {} protocol={} perms={}",
                plugin.plugin_id, plugin.version, enabled, plugin.protocol_version, permissions
            ));
            lines.push(format!(
                "  desc: {}",
                plugin.description.as_deref().unwrap_or("<none>")
            ));
            lines.push(format!("  dir: {}", plugin.install_dir));
        }
        lines.join("\n")
    };
    let plugin_controls = [
        "Plugin controls:",
        "- plugin.list",
        "- plugin.install <package_dir>",
        "- select plugin <id>",
        "- run plugin.enable [plugin_id]",
        "- run plugin.disable [plugin_id]",
        "- run --confirm plugin.remove [plugin_id]",
    ]
    .join("\n");

    format!(
        "Diagnostics Panel\nHost version: {}\nProtocol version: {}\nJournal entries: {}\nRunning: {}\nSucceeded: {}\nFailed: {}\nLast error: {}\nAvg duration(ms): {}\nSlowest op: {}\nActionable blockers: {}\nRuntime plugin health: {}\nRebase plan: {}\nRebase session: {}\n{}\n{}\n",
        host_version,
        protocol_version,
        entries.len(),
        started,
        succeeded,
        failed,
        last_error,
        avg_duration_ms,
        slowest,
        actionable_blockers,
        runtime_plugin_health,
        rebase_plan.unwrap_or_else(|| "<none>".to_string()),
        rebase_session.unwrap_or_else(|| "<none>".to_string()),
        installed_plugins,
        plugin_controls
    )
}

pub fn render_merge_dialog_preview(source_ref: &str, target_ref: &str, mode: &str) -> String {
    let impact = match mode {
        "ff" | "fast-forward" => "fast-forward only, aborts if history diverged",
        "no-ff" => "creates merge commit and preserves branch topology",
        "squash" => "squashes incoming changes without merge commit",
        _ => "unknown merge mode",
    };
    format!(
        "Merge dialog\nsource: {source_ref}\ntarget: {target_ref}\nmode: {mode}\nimpact: {impact}\nsafety: verify branch selection before continue\n"
    )
}

pub fn render_reset_dialog_preview(mode: &str, target: &str) -> String {
    let impact = match mode {
        "soft" => "moves HEAD only and keeps index/worktree",
        "mixed" => "moves HEAD and unstages index changes",
        "hard" => "moves HEAD and drops worktree changes",
        _ => "unknown reset mode",
    };
    let danger = if mode == "hard" {
        "danger: destructive operation, explicit confirm required"
    } else {
        "danger: moderate operation"
    };
    format!("Reset dialog\nmode: {mode}\ntarget: {target}\nimpact: {impact}\n{danger}\n")
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
                "tags.panel" => render_tags_panel(store),
                "compare.panel" => render_compare_panel(store),
                "diagnostics.panel" => render_diagnostics_panel(store),
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
                effects: plugin_api::ActionEffects::read_only(),
                confirm_policy: plugin_api::ConfirmPolicy::Never,
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
                effects: plugin_api::ActionEffects::read_only(),
                confirm_policy: plugin_api::ConfirmPolicy::Never,
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
                effects: plugin_api::ActionEffects::read_only(),
                confirm_policy: plugin_api::ConfirmPolicy::Never,
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
    fn renders_tags_panel_when_active_view_set() {
        let mut store = StateStore::new();
        store.set_active_view(Some("tags.panel".to_string()));
        store.update_repo(plugin_api::RepoSnapshot {
            root: "/tmp/demo".to_string(),
            head: Some("main".to_string()),
            conflict_state: None,
        });
        store.update_tags(vec![state_store::TagInfo {
            name: "v1.0.0".to_string(),
        }]);

        let rendered = render_window(&store, &palette::build_palette(&[], "", true));
        assert!(rendered.contains("Tags Panel"));
        assert!(rendered.contains("tags: v1.0.0"));
    }

    #[test]
    fn renders_compare_panel_when_active_view_set() {
        let mut store = StateStore::new();
        store.set_active_view(Some("compare.panel".to_string()));
        store.update_repo(plugin_api::RepoSnapshot {
            root: "/tmp/demo".to_string(),
            head: Some("main".to_string()),
            conflict_state: None,
        });
        store.update_compare_summary(
            "main".to_string(),
            "feature".to_string(),
            2,
            1,
            vec![state_store::CommitSummary {
                oid: "abc1234".to_string(),
                author: "Dev".to_string(),
                time: "now".to_string(),
                summary: "feat: compare".to_string(),
            }],
        );

        let rendered = render_window(&store, &palette::build_palette(&[], "", true));
        assert!(rendered.contains("Compare Panel"));
        assert!(rendered.contains("Base ref: main"));
        assert!(rendered.contains("Compare commits: abc1234 feat: compare"));
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
                effects: plugin_api::ActionEffects::read_only(),
                confirm_policy: plugin_api::ConfirmPolicy::Never,
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
                effects: plugin_api::ActionEffects::read_only(),
                confirm_policy: plugin_api::ConfirmPolicy::Never,
            }],
            "",
            true,
        );
        let rendered = render_window(&store, &palette_items);
        assert!(rendered.contains("Commit (on)"));
    }

    #[test]
    fn diagnostics_panel_shows_rebase_summary() {
        let mut store = StateStore::new();
        store.update_rebase_plan(state_store::RebasePlan {
            base_ref: "main".to_string(),
            base_oid: Some("abc".to_string()),
            entries: Vec::new(),
            affected_commit_count: 2,
            rewrite_types: vec!["pick".to_string()],
            published_history_warning: None,
            autosquash_aware: true,
        });
        store.update_rebase_session(state_store::RebaseSessionSnapshot {
            active: true,
            repo_root: Some("/tmp/repo".to_string()),
            base_ref: Some("main".to_string()),
            current_step: Some(1),
            total_steps: Some(2),
            blocking_conflict: false,
        });

        let rendered = render_diagnostics_panel(&store);
        assert!(rendered.contains("Host version:"));
        assert!(rendered.contains("Protocol version:"));
        assert!(rendered.contains("Runtime plugin health:"));
        assert!(rendered.contains("Rebase plan: base=main commits=2 autosquash=true"));
        assert!(rendered.contains("Rebase session: active=true step=1/2"));
    }

    #[test]
    fn diagnostics_panel_shows_performance_aggregates() {
        let mut store = StateStore::new();
        let entry_id = store.append_journal_entry(None, "history.search".to_string(), 100);
        store.finish_journal_entry(entry_id, state_store::JournalStatus::Succeeded, 160, None);
        let entry_id = store.append_journal_entry(None, "diff.worktree".to_string(), 200);
        store.finish_journal_entry(entry_id, state_store::JournalStatus::Failed, 320, None);

        let rendered = render_diagnostics_panel(&store);
        assert!(rendered.contains("Avg duration(ms):"));
        assert!(rendered.contains("Slowest op:"));
        assert!(rendered.contains("Actionable blockers:"));
    }

    #[test]
    fn diagnostics_panel_shows_installed_plugin_inventory() {
        let mut store = StateStore::new();
        store.update_plugin_status("status", state_store::PluginHealth::Ready);
        store.update_selected_plugin(Some("status".to_string()));
        store.update_installed_plugins(vec![state_store::InstalledPluginRecord {
            plugin_id: "status".to_string(),
            version: "0.1.0".to_string(),
            protocol_version: plugin_api::HOST_PLUGIN_PROTOCOL_VERSION.to_string(),
            enabled: true,
            description: Some("status plugin".to_string()),
            permissions: vec!["read_state".to_string()],
            install_dir: "/tmp/plugins/status".to_string(),
        }]);

        let rendered = render_diagnostics_panel(&store);
        assert!(rendered.contains("Runtime plugin health: status=ready"));
        assert!(rendered.contains("Selected plugin: status"));
        assert!(rendered.contains("Installed plugins: 1"));
        assert!(rendered.contains("* status v0.1.0 enabled"));
        assert!(rendered.contains("desc: status plugin"));
        assert!(rendered.contains("dir: /tmp/plugins/status"));
        assert!(rendered.contains("Plugin controls:"));
        assert!(rendered.contains("select plugin <id>"));
        assert!(rendered.contains("run --confirm plugin.remove [plugin_id]"));
    }

    #[test]
    fn status_panel_includes_keyboard_hints() {
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

        let rendered = render_status_panel(&store);
        assert!(rendered.contains("Keyboard hints: c=commit, a=amend"));
        assert!(rendered.contains("key:c"));
    }

    #[test]
    fn render_diff_panel_shows_stage_and_discard_hunk_actions_for_worktree() {
        let mut store = StateStore::new();
        store.update_diff(state_store::DiffState {
            source: Some(state_store::DiffSource::Worktree {
                paths: vec!["file.txt".to_string()],
            }),
            descriptor: None,
            load_request: None,
            chunks: Vec::new(),
            content: Some("diff --git a/file.txt b/file.txt".to_string()),
            hunks: vec![state_store::DiffHunk {
                file_path: "file.txt".to_string(),
                hunk_index: 0,
                header: "@@ -1 +1 @@".to_string(),
                lines: vec!["-old".to_string(), "+new".to_string()],
            }],
            loading: false,
            error: None,
        });

        let rendered = render_diff_panel(&store);
        assert!(rendered.contains("index.stage_hunk file.txt 0"));
        assert!(rendered.contains("file.discard_hunk file.txt 0"));
    }

    #[test]
    fn renders_merge_dialog_preview_with_safety_copy() {
        let text = render_merge_dialog_preview("feature", "main", "no-ff");
        assert!(text.contains("Merge dialog"));
        assert!(text.contains("source: feature"));
        assert!(text.contains("target: main"));
        assert!(text.contains("mode: no-ff"));
        assert!(text.contains("safety:"));
    }

    #[test]
    fn renders_reset_dialog_preview_with_explicit_hard_danger() {
        let text = render_reset_dialog_preview("hard", "HEAD~1");
        assert!(text.contains("Reset dialog"));
        assert!(text.contains("mode: hard"));
        assert!(text.contains("target: HEAD~1"));
        assert!(text.contains("drops worktree changes"));
        assert!(text.contains("explicit confirm required"));
    }
}
