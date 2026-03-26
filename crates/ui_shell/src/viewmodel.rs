use state_store::StoreSnapshot;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ItemsRef {
    Staged,
    Unstaged,
    Untracked,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ViewNode {
    Container {
        children: Vec<ViewNode>,
    },
    Text {
        value: String,
    },
    List {
        title: String,
        items_ref: ItemsRef,
    },
    HistoryList {
        title: String,
    },
    CommitDetails,
    BranchList {
        title: String,
    },
    TagList {
        title: String,
    },
    Button {
        label: String,
        on_action: String,
        enabled_when: bool,
        shortcut_hint: Option<String>,
        accessibility_label: Option<String>,
    },
}

pub fn build_status_panel(snapshot: &StoreSnapshot) -> ViewNode {
    let current_branch = snapshot
        .repo
        .as_ref()
        .and_then(|repo| repo.head.as_deref())
        .unwrap_or("<unknown>");
    let commit_line = if snapshot.commit_message.draft.is_empty() {
        "Commit message: <empty>".to_string()
    } else {
        format!("Commit message: {}", snapshot.commit_message.draft)
    };
    let commit_error = snapshot
        .commit_message
        .error
        .as_ref()
        .map(|err| format!("Commit error: {err}"));
    let operation_banner = snapshot.repo.as_ref().and_then(|repo| {
        repo.conflict_state.as_ref().map(|state| {
            let label = match state {
                plugin_api::ConflictState::Merge => "merge",
                plugin_api::ConflictState::Rebase => "rebase",
                plugin_api::ConflictState::CherryPick => "cherry-pick",
            };
            format!("Operation in progress: {label}")
        })
    });
    let recovery_prompt = snapshot.repo.as_ref().and_then(|repo| {
        repo.conflict_state.as_ref().map(|state| {
            let label = match state {
                plugin_api::ConflictState::Merge => "merge",
                plugin_api::ConflictState::Rebase => "rebase",
                plugin_api::ConflictState::CherryPick => "cherry-pick",
            };
            format!("Recovery prompt: unfinished {label} detected (continue or abort).")
        })
    });
    let running_ops = snapshot
        .journal
        .entries
        .iter()
        .filter(|entry| matches!(entry.status, state_store::JournalStatus::Started))
        .map(|entry| entry.op.as_str())
        .collect::<Vec<_>>();
    let loading_line = if running_ops.is_empty() {
        None
    } else {
        Some(format!("Loading: {}", running_ops.join(", ")))
    };
    let operation_error = snapshot
        .journal
        .entries
        .iter()
        .rev()
        .find(|entry| matches!(entry.status, state_store::JournalStatus::Failed))
        .map(|entry| {
            let detail = entry.error.as_deref().unwrap_or("unknown error");
            format!("Operation error: {}: {}", entry.op, detail)
        });
    let session_badge = snapshot.journal.entries.iter().rev().find_map(|entry| {
        let session_id = entry.session_id?;
        let state = match entry.session_state.as_ref() {
            Some(state_store::OperationSessionState::Running) => "running",
            Some(state_store::OperationSessionState::Succeeded) => "succeeded",
            Some(state_store::OperationSessionState::Failed) => "failed",
            None => "unknown",
        };
        Some(format!("Session #{session_id}: {} ({state})", entry.op))
    });

    let mut children = vec![
        ViewNode::Text {
            value: "Status Panel".to_string(),
        },
        ViewNode::Text {
            value: format!("Current branch: {current_branch}"),
        },
        ViewNode::List {
            title: "staged".to_string(),
            items_ref: ItemsRef::Staged,
        },
        ViewNode::List {
            title: "unstaged".to_string(),
            items_ref: ItemsRef::Unstaged,
        },
        ViewNode::List {
            title: "untracked".to_string(),
            items_ref: ItemsRef::Untracked,
        },
        ViewNode::Text { value: commit_line },
    ];
    if let Some(error) = commit_error {
        children.push(ViewNode::Text { value: error });
    }
    if let Some(banner) = operation_banner {
        children.push(ViewNode::Text { value: banner });
    }
    if let Some(prompt) = recovery_prompt {
        children.push(ViewNode::Text { value: prompt });
    }
    if let Some(loading) = loading_line {
        children.push(ViewNode::Text { value: loading });
    }
    if let Some(session) = session_badge {
        children.push(ViewNode::Text { value: session });
    }
    if let Some(error) = operation_error {
        children.push(ViewNode::Text { value: error });
    }
    children.push(ViewNode::Button {
        label: "Commit".to_string(),
        on_action: "commit.create".to_string(),
        enabled_when: !snapshot.status.staged.is_empty(),
        shortcut_hint: Some("c".to_string()),
        accessibility_label: Some("Commit staged changes".to_string()),
    });
    children.push(ViewNode::Button {
        label: "Amend".to_string(),
        on_action: "commit.amend".to_string(),
        enabled_when: snapshot.repo.is_some(),
        shortcut_hint: Some("a".to_string()),
        accessibility_label: Some("Amend latest commit".to_string()),
    });
    children.push(ViewNode::Text {
        value: "Keyboard hints: c=commit, a=amend".to_string(),
    });

    ViewNode::Container { children }
}

pub fn build_history_panel(snapshot: &StoreSnapshot) -> ViewNode {
    let has_commits = !snapshot.history.commits.is_empty();
    let header = if has_commits {
        "History Panel".to_string()
    } else {
        "History Panel (empty)".to_string()
    };

    let filter_line = format!(
        "Filters: author={}, text={}",
        snapshot.history.filter_author.as_deref().unwrap_or("<any>"),
        snapshot.history.filter_text.as_deref().unwrap_or("<any>")
    );

    let current_branch = snapshot
        .repo
        .as_ref()
        .and_then(|repo| repo.head.as_deref())
        .unwrap_or("<unknown>");

    let mut children = vec![ViewNode::Text { value: header }];
    children.push(ViewNode::Text {
        value: format!("Current branch: {current_branch}"),
    });
    if snapshot.history.loading {
        children.push(ViewNode::Text {
            value: "History: loading...".to_string(),
        });
    }
    if let Some(error) = snapshot.history.error.as_ref() {
        children.push(ViewNode::Text {
            value: format!("History error: {error}"),
        });
    }
    children.push(ViewNode::Text { value: filter_line });
    children.push(ViewNode::HistoryList {
        title: "commits".to_string(),
    });
    children.push(ViewNode::CommitDetails);
    children.push(ViewNode::Button {
        label: "Load more".to_string(),
        on_action: "history.load_more".to_string(),
        enabled_when: snapshot.history_can_load_more(),
        shortcut_hint: Some("l".to_string()),
        accessibility_label: Some("Load next history page".to_string()),
    });
    children.push(ViewNode::Button {
        label: "Show commit".to_string(),
        on_action: "history.select_commit".to_string(),
        enabled_when: snapshot.history_has_selection(),
        shortcut_hint: Some("enter".to_string()),
        accessibility_label: Some("Open selected commit details".to_string()),
    });
    children.push(ViewNode::Button {
        label: "Cherry-pick".to_string(),
        on_action: "cherry_pick.commit".to_string(),
        enabled_when: snapshot.history_has_selection(),
        shortcut_hint: Some("p".to_string()),
        accessibility_label: Some("Cherry-pick selected commit".to_string()),
    });
    children.push(ViewNode::Button {
        label: "Revert".to_string(),
        on_action: "revert.commit".to_string(),
        enabled_when: snapshot.history_has_selection(),
        shortcut_hint: Some("r".to_string()),
        accessibility_label: Some("Revert selected commit".to_string()),
    });
    children.push(ViewNode::Text {
        value: "Keyboard hints: l=load more, enter=show commit, p=cherry-pick, r=revert"
            .to_string(),
    });

    ViewNode::Container { children }
}

pub fn build_branches_panel(snapshot: &StoreSnapshot) -> ViewNode {
    let has_branches = !snapshot.branches.branches.is_empty();
    let has_tags = !snapshot.tags.tags.is_empty();
    let compare_line = match (
        snapshot.compare.base_ref.as_deref(),
        snapshot.compare.head_ref.as_deref(),
    ) {
        (Some(base_ref), Some(head_ref)) => Some(format!("Compare: {base_ref} -> {head_ref}")),
        _ => None,
    };
    let operation_banner = snapshot.repo.as_ref().and_then(|repo| {
        repo.conflict_state.as_ref().map(|state| {
            let label = match state {
                plugin_api::ConflictState::Merge => "merge",
                plugin_api::ConflictState::Rebase => "rebase",
                plugin_api::ConflictState::CherryPick => "cherry-pick",
            };
            format!("Operation in progress: {label}")
        })
    });
    let conflict_route = snapshot
        .repo
        .as_ref()
        .and_then(|repo| match repo.conflict_state {
            Some(plugin_api::ConflictState::Merge) => {
                Some("Conflict route: resolve conflicts or run merge.abort".to_string())
            }
            Some(plugin_api::ConflictState::Rebase) => {
                Some("Conflict route: resolve conflicts or run rebase.abort".to_string())
            }
            Some(plugin_api::ConflictState::CherryPick) => {
                Some("Conflict route: resolve conflicts or run cherry_pick.abort".to_string())
            }
            _ => None,
        });
    let session_badge = snapshot.journal.entries.iter().rev().find_map(|entry| {
        let session_id = entry.session_id?;
        let state = match entry.session_state.as_ref() {
            Some(state_store::OperationSessionState::Running) => "running",
            Some(state_store::OperationSessionState::Succeeded) => "succeeded",
            Some(state_store::OperationSessionState::Failed) => "failed",
            None => "unknown",
        };
        Some(format!("Session #{session_id}: {} ({state})", entry.op))
    });
    let current_branch = snapshot
        .branches
        .branches
        .iter()
        .find(|branch| branch.is_current)
        .map(|branch| branch.name.as_str())
        .or_else(|| snapshot.repo.as_ref().and_then(|repo| repo.head.as_deref()))
        .unwrap_or("<unknown>");

    let mut children = vec![
        ViewNode::Text {
            value: "Branches Panel".to_string(),
        },
        ViewNode::Text {
            value: format!("Current branch: {current_branch}"),
        },
        ViewNode::BranchList {
            title: "branches".to_string(),
        },
        ViewNode::TagList {
            title: "tags".to_string(),
        },
    ];

    if let Some(line) = compare_line {
        children.push(ViewNode::Text { value: line });
        children.push(ViewNode::Text {
            value: format!(
                "Ahead/behind: +{} / -{}",
                snapshot.compare.ahead, snapshot.compare.behind
            ),
        });
        if !snapshot.compare.commits.is_empty() {
            let preview = snapshot
                .compare
                .commits
                .iter()
                .take(5)
                .map(|commit| {
                    let short = commit.oid.chars().take(7).collect::<String>();
                    format!("{short} {}", commit.summary)
                })
                .collect::<Vec<_>>()
                .join(", ");
            children.push(ViewNode::Text {
                value: format!("Compare commits: {preview}"),
            });
        }
    }

    if let Some(banner) = operation_banner {
        children.push(ViewNode::Text { value: banner });
    }
    if let Some(route) = conflict_route {
        children.push(ViewNode::Text { value: route });
    }
    if let Some(session) = session_badge {
        children.push(ViewNode::Text { value: session });
    }

    children.push(ViewNode::Text {
        value: "Merge safety: ensure clean worktree before merge/cherry-pick/rebase.".to_string(),
    });
    children.push(ViewNode::Text {
        value: "Reset safety: reset --hard is destructive and requires explicit confirm"
            .to_string(),
    });

    if let Some(plan) = snapshot.rebase.plan.as_ref() {
        children.push(ViewNode::Text {
            value: format!(
                "Rebase plan: base={} commits={} autosquash={}",
                plan.base_ref, plan.affected_commit_count, plan.autosquash_aware
            ),
        });
        if !plan.rewrite_types.is_empty() {
            children.push(ViewNode::Text {
                value: format!("Rebase rewrite types: {}", plan.rewrite_types.join(", ")),
            });
        }
        if let Some(warning) = plan.published_history_warning.as_ref() {
            children.push(ViewNode::Text {
                value: format!("Rebase warning: {warning}"),
            });
        }
    }

    if let Some(session) = snapshot.rebase.session.as_ref() {
        let step = session
            .current_step
            .map(|value| value.to_string())
            .unwrap_or_else(|| "?".to_string());
        let total = session
            .total_steps
            .map(|value| value.to_string())
            .unwrap_or_else(|| "?".to_string());
        children.push(ViewNode::Text {
            value: format!(
                "Rebase session: active={} step={step}/{total}",
                session.active
            ),
        });
        children.push(ViewNode::Text {
            value: "Rebase controls: continue / skip / abort".to_string(),
        });
    }

    children.push(ViewNode::Button {
        label: "Create Rebase Plan".to_string(),
        on_action: "rebase.plan.create".to_string(),
        enabled_when: snapshot.repo.is_some(),
        shortcut_hint: None,
        accessibility_label: Some("Create interactive rebase plan".to_string()),
    });
    children.push(ViewNode::Button {
        label: "Execute Rebase".to_string(),
        on_action: "rebase.execute".to_string(),
        enabled_when: snapshot.rebase.plan.is_some(),
        shortcut_hint: None,
        accessibility_label: Some("Execute rebase plan".to_string()),
    });
    children.push(ViewNode::Button {
        label: "Continue Rebase".to_string(),
        on_action: "rebase.continue".to_string(),
        enabled_when: snapshot.rebase.session.is_some(),
        shortcut_hint: None,
        accessibility_label: Some("Continue active rebase".to_string()),
    });
    children.push(ViewNode::Button {
        label: "Skip Rebase Commit".to_string(),
        on_action: "rebase.skip".to_string(),
        enabled_when: snapshot.rebase.session.is_some(),
        shortcut_hint: None,
        accessibility_label: Some("Skip current commit in rebase".to_string()),
    });
    children.push(ViewNode::Button {
        label: "Abort Rebase".to_string(),
        on_action: "rebase.abort".to_string(),
        enabled_when: snapshot.rebase.session.is_some(),
        shortcut_hint: None,
        accessibility_label: Some("Abort active rebase".to_string()),
    });

    if !has_branches {
        children.push(ViewNode::Text {
            value: "Branches: empty".to_string(),
        });
    }
    if !has_tags {
        children.push(ViewNode::Text {
            value: "Tags: empty".to_string(),
        });
    }

    ViewNode::Container { children }
}

pub fn build_tags_panel(snapshot: &StoreSnapshot) -> ViewNode {
    let current_branch = snapshot
        .repo
        .as_ref()
        .and_then(|repo| repo.head.as_deref())
        .unwrap_or("<unknown>");
    let selected_branch = snapshot
        .selection
        .selected_branch
        .as_deref()
        .unwrap_or("<none>");

    let mut children = vec![
        ViewNode::Text {
            value: "Tags Panel".to_string(),
        },
        ViewNode::Text {
            value: format!("Current branch: {current_branch}"),
        },
        ViewNode::Text {
            value: format!("Selected branch context: {selected_branch}"),
        },
        ViewNode::TagList {
            title: "tags".to_string(),
        },
        ViewNode::Button {
            label: "Create Tag".to_string(),
            on_action: "tag.create".to_string(),
            enabled_when: snapshot.repo.is_some(),
            shortcut_hint: None,
            accessibility_label: Some("Create tag from current HEAD".to_string()),
        },
        ViewNode::Text {
            value: "Tag operations: use `run tag.delete <name>` or `run tag.checkout <name>`."
                .to_string(),
        },
    ];

    if snapshot.tags.tags.is_empty() {
        children.push(ViewNode::Text {
            value: "Tags: empty".to_string(),
        });
    }

    ViewNode::Container { children }
}

pub fn build_compare_panel(snapshot: &StoreSnapshot) -> ViewNode {
    let base_ref = snapshot.compare.base_ref.as_deref().unwrap_or("<unset>");
    let head_ref = snapshot.compare.head_ref.as_deref().unwrap_or("<unset>");
    let selected_branch = snapshot
        .selection
        .selected_branch
        .as_deref()
        .unwrap_or("<none>");

    let mut children = vec![
        ViewNode::Text {
            value: "Compare Panel".to_string(),
        },
        ViewNode::Text {
            value: format!("Base ref: {base_ref}"),
        },
        ViewNode::Text {
            value: format!("Head ref: {head_ref}"),
        },
        ViewNode::Text {
            value: format!("Selected branch context: {selected_branch}"),
        },
        ViewNode::Text {
            value: format!(
                "Ahead/behind: +{} / -{}",
                snapshot.compare.ahead, snapshot.compare.behind
            ),
        },
        ViewNode::Button {
            label: "Compare Branches".to_string(),
            on_action: "compare.refs".to_string(),
            enabled_when: snapshot.repo.is_some(),
            shortcut_hint: None,
            accessibility_label: Some("Compare two refs".to_string()),
        },
    ];

    if snapshot.compare.commits.is_empty() {
        children.push(ViewNode::Text {
            value: "Compare commits: <empty>".to_string(),
        });
    } else {
        let preview = snapshot
            .compare
            .commits
            .iter()
            .take(10)
            .map(|commit| {
                let short = commit.oid.chars().take(7).collect::<String>();
                format!("{short} {}", commit.summary)
            })
            .collect::<Vec<_>>()
            .join(", ");
        children.push(ViewNode::Text {
            value: format!("Compare commits: {preview}"),
        });
    }

    ViewNode::Container { children }
}

pub fn render(node: &ViewNode, snapshot: &StoreSnapshot) -> String {
    let mut out = String::new();
    render_into(node, snapshot, 0, &mut out);
    out
}

fn render_into(node: &ViewNode, snapshot: &StoreSnapshot, level: usize, out: &mut String) {
    let indent = "  ".repeat(level);
    match node {
        ViewNode::Container { children } => {
            for child in children {
                render_into(child, snapshot, level + 1, out);
            }
        }
        ViewNode::Text { value } => {
            out.push_str(&format!("{indent}{value}\n"));
        }
        ViewNode::List { title, items_ref } => {
            let items = match items_ref {
                ItemsRef::Staged => &snapshot.status.staged,
                ItemsRef::Unstaged => &snapshot.status.unstaged,
                ItemsRef::Untracked => &snapshot.status.untracked,
            };
            let list = items.join(", ");
            out.push_str(&format!("{indent}{title}: {list}\n"));
        }
        ViewNode::HistoryList { title } => {
            let list = snapshot
                .history
                .commits
                .iter()
                .map(|commit| {
                    let short = commit.oid.chars().take(7).collect::<String>();
                    let marker = if snapshot
                        .selection
                        .selected_commit_oid
                        .as_deref()
                        .map(|selected| selected == commit.oid)
                        .unwrap_or(false)
                    {
                        "*"
                    } else {
                        " "
                    };
                    format!("{marker}{short} {}", commit.summary)
                })
                .collect::<Vec<_>>()
                .join(", ");
            out.push_str(&format!("{indent}{title}: {list}\n"));
        }
        ViewNode::CommitDetails => {
            let selected = snapshot
                .selection
                .selected_commit_oid
                .as_deref()
                .unwrap_or("<none>");
            if let Some(details) = snapshot.commit_cache.get(selected) {
                out.push_str(&format!("{indent}Commit: {}\n", details.message));
            } else {
                out.push_str(&format!("{indent}Commit: {selected}\n"));
            }
        }
        ViewNode::BranchList { title } => {
            let list = snapshot
                .branches
                .branches
                .iter()
                .map(|branch| {
                    let marker = if branch.is_current { "*" } else { " " };
                    format!("{marker}{}", branch.name)
                })
                .collect::<Vec<_>>()
                .join(", ");
            out.push_str(&format!("{indent}{title}: {list}\n"));
        }
        ViewNode::TagList { title } => {
            let list = snapshot
                .tags
                .tags
                .iter()
                .map(|tag| tag.name.clone())
                .collect::<Vec<_>>()
                .join(", ");
            out.push_str(&format!("{indent}{title}: {list}\n"));
        }
        ViewNode::Button {
            label,
            on_action,
            enabled_when,
            shortcut_hint,
            accessibility_label,
        } => {
            let state = if *enabled_when { "enabled" } else { "disabled" };
            let shortcut = shortcut_hint
                .as_ref()
                .map(|value| format!(" key:{value}"))
                .unwrap_or_default();
            let a11y = accessibility_label
                .as_ref()
                .map(|value| format!(" a11y:{value}"))
                .unwrap_or_default();
            out.push_str(&format!(
                "{indent}[{label}] {state} -> {on_action}{shortcut}{a11y}\n"
            ));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use state_store::{SelectionState, StatusSnapshot, StoreSnapshot};

    #[test]
    fn builds_panel_with_commit_button_state() {
        let snapshot = StoreSnapshot {
            repo: None,
            status: StatusSnapshot {
                staged: vec!["src/lib.rs".to_string()],
                unstaged: Vec::new(),
                untracked: Vec::new(),
            },
            selection: SelectionState::default(),
            history: state_store::HistoryState::default(),
            commit_cache: std::collections::HashMap::new(),
            diff: state_store::DiffState::default(),
            compare: state_store::CompareState::default(),
            branches: state_store::BranchesState::default(),
            tags: state_store::TagsState::default(),
            commit_message: state_store::CommitMessageState::default(),
            rebase: state_store::RebaseState::default(),
            journal: state_store::OperationJournalState::default(),
            active_view: None,
            plugins: Vec::new(),
            installed_plugins: Vec::new(),
            version: 1,
        };

        let panel = build_status_panel(&snapshot);
        let rendered = render(&panel, &snapshot);
        assert!(rendered.contains("Status Panel"));
        assert!(rendered.contains("[Commit] enabled"));
    }

    #[test]
    fn status_panel_shows_current_branch() {
        let snapshot = StoreSnapshot {
            repo: Some(plugin_api::RepoSnapshot {
                root: "./demo".to_string(),
                head: Some("feature/demo".to_string()),
                conflict_state: None,
            }),
            status: StatusSnapshot::default(),
            selection: SelectionState::default(),
            history: state_store::HistoryState::default(),
            commit_cache: std::collections::HashMap::new(),
            diff: state_store::DiffState::default(),
            compare: state_store::CompareState::default(),
            branches: state_store::BranchesState::default(),
            tags: state_store::TagsState::default(),
            commit_message: state_store::CommitMessageState::default(),
            rebase: state_store::RebaseState::default(),
            journal: state_store::OperationJournalState::default(),
            active_view: None,
            plugins: Vec::new(),
            installed_plugins: Vec::new(),
            version: 1,
        };

        let panel = build_status_panel(&snapshot);
        let rendered = render(&panel, &snapshot);
        assert!(rendered.contains("Current branch: feature/demo"));
    }

    #[test]
    fn status_panel_shows_recovery_prompt_when_conflict_active() {
        let snapshot = StoreSnapshot {
            repo: Some(plugin_api::RepoSnapshot {
                root: "./demo".to_string(),
                head: Some("main".to_string()),
                conflict_state: Some(plugin_api::ConflictState::Merge),
            }),
            status: StatusSnapshot::default(),
            selection: SelectionState::default(),
            history: state_store::HistoryState::default(),
            commit_cache: std::collections::HashMap::new(),
            diff: state_store::DiffState::default(),
            compare: state_store::CompareState::default(),
            branches: state_store::BranchesState::default(),
            tags: state_store::TagsState::default(),
            commit_message: state_store::CommitMessageState::default(),
            rebase: state_store::RebaseState::default(),
            journal: state_store::OperationJournalState::default(),
            active_view: None,
            plugins: Vec::new(),
            installed_plugins: Vec::new(),
            version: 1,
        };

        let panel = build_status_panel(&snapshot);
        let rendered = render(&panel, &snapshot);
        assert!(rendered.contains("Recovery prompt: unfinished merge detected"));
    }

    #[test]
    fn shows_loading_and_operation_error_from_journal() {
        let snapshot = StoreSnapshot {
            repo: None,
            status: StatusSnapshot {
                staged: vec!["src/lib.rs".to_string()],
                unstaged: Vec::new(),
                untracked: Vec::new(),
            },
            selection: SelectionState::default(),
            history: state_store::HistoryState::default(),
            commit_cache: std::collections::HashMap::new(),
            diff: state_store::DiffState::default(),
            compare: state_store::CompareState::default(),
            branches: state_store::BranchesState::default(),
            tags: state_store::TagsState::default(),
            commit_message: state_store::CommitMessageState::default(),
            rebase: state_store::RebaseState::default(),
            journal: state_store::OperationJournalState {
                entries: vec![
                    state_store::OperationJournalEntry {
                        id: 1,
                        job_id: None,
                        op: "status.refresh".to_string(),
                        status: state_store::JournalStatus::Started,
                        started_at_ms: 1,
                        finished_at_ms: None,
                        error: None,
                        session_id: Some(10),
                        session_kind: Some(
                            state_store::OperationSessionKind::AdvancedBranchOperation,
                        ),
                        session_state: Some(state_store::OperationSessionState::Running),
                        pre_refs: None,
                        post_refs: None,
                    },
                    state_store::OperationJournalEntry {
                        id: 2,
                        job_id: None,
                        op: "commit.create".to_string(),
                        status: state_store::JournalStatus::Failed,
                        started_at_ms: 2,
                        finished_at_ms: Some(3),
                        error: Some("nothing to commit".to_string()),
                        session_id: None,
                        session_kind: None,
                        session_state: None,
                        pre_refs: None,
                        post_refs: None,
                    },
                ],
            },
            active_view: None,
            plugins: Vec::new(),
            installed_plugins: Vec::new(),
            version: 1,
        };

        let panel = build_status_panel(&snapshot);
        let rendered = render(&panel, &snapshot);
        assert!(rendered.contains("Loading: status.refresh"));
        assert!(rendered.contains("Session #10: status.refresh (running)"));
        assert!(rendered.contains("Operation error: commit.create: nothing to commit"));
    }

    #[test]
    fn branches_panel_shows_safety_copy_and_conflict_route() {
        let snapshot = StoreSnapshot {
            repo: Some(plugin_api::RepoSnapshot {
                root: "./demo".to_string(),
                head: Some("main".to_string()),
                conflict_state: Some(plugin_api::ConflictState::Merge),
            }),
            status: StatusSnapshot::default(),
            selection: SelectionState::default(),
            history: state_store::HistoryState::default(),
            commit_cache: std::collections::HashMap::new(),
            diff: state_store::DiffState::default(),
            compare: state_store::CompareState::default(),
            branches: state_store::BranchesState::default(),
            tags: state_store::TagsState::default(),
            commit_message: state_store::CommitMessageState::default(),
            rebase: state_store::RebaseState::default(),
            journal: state_store::OperationJournalState {
                entries: vec![state_store::OperationJournalEntry {
                    id: 1,
                    job_id: None,
                    op: "merge.execute".to_string(),
                    status: state_store::JournalStatus::Started,
                    started_at_ms: 1,
                    finished_at_ms: None,
                    error: None,
                    session_id: Some(77),
                    session_kind: Some(state_store::OperationSessionKind::AdvancedBranchOperation),
                    session_state: Some(state_store::OperationSessionState::Running),
                    pre_refs: None,
                    post_refs: None,
                }],
            },
            active_view: None,
            plugins: Vec::new(),
            installed_plugins: Vec::new(),
            version: 1,
        };

        let panel = build_branches_panel(&snapshot);
        let rendered = render(&panel, &snapshot);
        assert!(rendered.contains("Operation in progress: merge"));
        assert!(rendered.contains("Conflict route: resolve conflicts or run merge.abort"));
        assert!(rendered.contains("Session #77: merge.execute (running)"));
        assert!(rendered.contains("Merge safety:"));
        assert!(rendered.contains("Reset safety:"));
        assert!(rendered.contains("reset --hard is destructive"));
    }

    #[test]
    fn branches_panel_shows_rebase_plan_and_session() {
        let snapshot = StoreSnapshot {
            repo: Some(plugin_api::RepoSnapshot {
                root: "./demo".to_string(),
                head: Some("feature".to_string()),
                conflict_state: Some(plugin_api::ConflictState::Rebase),
            }),
            status: StatusSnapshot::default(),
            selection: SelectionState::default(),
            history: state_store::HistoryState::default(),
            commit_cache: std::collections::HashMap::new(),
            diff: state_store::DiffState::default(),
            compare: state_store::CompareState::default(),
            branches: state_store::BranchesState::default(),
            tags: state_store::TagsState::default(),
            commit_message: state_store::CommitMessageState::default(),
            rebase: state_store::RebaseState {
                plan: Some(state_store::RebasePlan {
                    base_ref: "main".to_string(),
                    base_oid: Some("abc".to_string()),
                    entries: Vec::new(),
                    affected_commit_count: 3,
                    rewrite_types: vec!["pick".to_string(), "squash".to_string()],
                    published_history_warning: Some("published branch".to_string()),
                    autosquash_aware: true,
                }),
                session: Some(state_store::RebaseSessionSnapshot {
                    active: true,
                    repo_root: Some("./demo".to_string()),
                    base_ref: Some("main".to_string()),
                    current_step: Some(1),
                    total_steps: Some(3),
                    blocking_conflict: true,
                }),
            },
            journal: state_store::OperationJournalState::default(),
            active_view: None,
            plugins: Vec::new(),
            installed_plugins: Vec::new(),
            version: 1,
        };

        let panel = build_branches_panel(&snapshot);
        let rendered = render(&panel, &snapshot);
        assert!(rendered.contains("Rebase plan: base=main commits=3 autosquash=true"));
        assert!(rendered.contains("Rebase rewrite types: pick, squash"));
        assert!(rendered.contains("Rebase warning: published branch"));
        assert!(rendered.contains("Rebase session: active=true step=1/3"));
        assert!(rendered.contains("Rebase controls: continue / skip / abort"));
        assert!(rendered.contains("[Create Rebase Plan] enabled -> rebase.plan.create"));
        assert!(rendered.contains("[Execute Rebase] enabled -> rebase.execute"));
        assert!(rendered.contains("[Continue Rebase] enabled -> rebase.continue"));
        assert!(rendered.contains("[Skip Rebase Commit] enabled -> rebase.skip"));
        assert!(rendered.contains("[Abort Rebase] enabled -> rebase.abort"));
    }

    #[test]
    fn tags_panel_shows_tag_list_and_actions() {
        let snapshot = StoreSnapshot {
            repo: Some(plugin_api::RepoSnapshot {
                root: "./demo".to_string(),
                head: Some("main".to_string()),
                conflict_state: None,
            }),
            status: StatusSnapshot::default(),
            selection: SelectionState {
                selected_paths: Vec::new(),
                selected_commit_oid: None,
                selected_branch: Some("release".to_string()),
                selected_plugin_id: None,
            },
            history: state_store::HistoryState::default(),
            commit_cache: std::collections::HashMap::new(),
            diff: state_store::DiffState::default(),
            compare: state_store::CompareState::default(),
            branches: state_store::BranchesState::default(),
            tags: state_store::TagsState {
                tags: vec![state_store::TagInfo {
                    name: "v1.0.0".to_string(),
                }],
            },
            commit_message: state_store::CommitMessageState::default(),
            rebase: state_store::RebaseState::default(),
            journal: state_store::OperationJournalState::default(),
            active_view: None,
            plugins: Vec::new(),
            installed_plugins: Vec::new(),
            version: 1,
        };

        let panel = build_tags_panel(&snapshot);
        let rendered = render(&panel, &snapshot);
        assert!(rendered.contains("Tags Panel"));
        assert!(rendered.contains("Selected branch context: release"));
        assert!(rendered.contains("tags: v1.0.0"));
        assert!(rendered.contains("[Create Tag] enabled -> tag.create"));
    }

    #[test]
    fn compare_panel_shows_summary_and_action() {
        let snapshot = StoreSnapshot {
            repo: Some(plugin_api::RepoSnapshot {
                root: "./demo".to_string(),
                head: Some("main".to_string()),
                conflict_state: None,
            }),
            status: StatusSnapshot::default(),
            selection: SelectionState {
                selected_paths: Vec::new(),
                selected_commit_oid: None,
                selected_branch: Some("feature".to_string()),
                selected_plugin_id: None,
            },
            history: state_store::HistoryState::default(),
            commit_cache: std::collections::HashMap::new(),
            diff: state_store::DiffState::default(),
            compare: state_store::CompareState {
                base_ref: Some("main".to_string()),
                head_ref: Some("feature".to_string()),
                ahead: 2,
                behind: 1,
                commits: vec![state_store::CommitSummary {
                    oid: "abc1234".to_string(),
                    author: "Dev".to_string(),
                    time: "now".to_string(),
                    summary: "feat: compare".to_string(),
                }],
            },
            branches: state_store::BranchesState::default(),
            tags: state_store::TagsState::default(),
            commit_message: state_store::CommitMessageState::default(),
            rebase: state_store::RebaseState::default(),
            journal: state_store::OperationJournalState::default(),
            active_view: None,
            plugins: Vec::new(),
            installed_plugins: Vec::new(),
            version: 1,
        };

        let panel = build_compare_panel(&snapshot);
        let rendered = render(&panel, &snapshot);
        assert!(rendered.contains("Compare Panel"));
        assert!(rendered.contains("Base ref: main"));
        assert!(rendered.contains("Head ref: feature"));
        assert!(rendered.contains("Ahead/behind: +2 / -1"));
        assert!(rendered.contains("Compare commits: abc1234 feat: compare"));
        assert!(rendered.contains("[Compare Branches] enabled -> compare.refs"));
    }

    #[test]
    fn history_panel_shows_rewrite_entry_points() {
        let snapshot = StoreSnapshot {
            repo: Some(plugin_api::RepoSnapshot {
                root: "./demo".to_string(),
                head: Some("main".to_string()),
                conflict_state: None,
            }),
            status: StatusSnapshot::default(),
            selection: SelectionState {
                selected_paths: Vec::new(),
                selected_commit_oid: Some("abc123".to_string()),
                selected_branch: None,
                selected_plugin_id: None,
            },
            history: state_store::HistoryState {
                commits: vec![state_store::CommitSummary {
                    oid: "abc123".to_string(),
                    author: "Dev".to_string(),
                    time: "now".to_string(),
                    summary: "feat: one".to_string(),
                }],
                ..state_store::HistoryState::default()
            },
            commit_cache: std::collections::HashMap::new(),
            diff: state_store::DiffState::default(),
            compare: state_store::CompareState::default(),
            branches: state_store::BranchesState::default(),
            tags: state_store::TagsState::default(),
            commit_message: state_store::CommitMessageState::default(),
            rebase: state_store::RebaseState::default(),
            journal: state_store::OperationJournalState::default(),
            active_view: None,
            plugins: Vec::new(),
            installed_plugins: Vec::new(),
            version: 1,
        };

        let panel = build_history_panel(&snapshot);
        let rendered = render(&panel, &snapshot);
        assert!(rendered.contains("[Cherry-pick] enabled -> cherry_pick.commit"));
        assert!(rendered.contains("[Revert] enabled -> revert.commit"));
    }
}
