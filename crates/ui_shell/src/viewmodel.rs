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
    },
}

pub fn build_status_panel(snapshot: &StoreSnapshot) -> ViewNode {
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

    let mut children = vec![
        ViewNode::Text {
            value: "Status Panel".to_string(),
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
    if let Some(loading) = loading_line {
        children.push(ViewNode::Text { value: loading });
    }
    if let Some(error) = operation_error {
        children.push(ViewNode::Text { value: error });
    }
    children.push(ViewNode::Button {
        label: "Commit".to_string(),
        on_action: "commit.create".to_string(),
        enabled_when: !snapshot.status.staged.is_empty(),
    });
    children.push(ViewNode::Button {
        label: "Amend".to_string(),
        on_action: "commit.amend".to_string(),
        enabled_when: snapshot.repo.is_some(),
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

    let mut children = vec![ViewNode::Text { value: header }];
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
    });
    children.push(ViewNode::Button {
        label: "Show commit".to_string(),
        on_action: "history.select_commit".to_string(),
        enabled_when: snapshot.history_has_selection(),
    });

    ViewNode::Container { children }
}

pub fn build_branches_panel(snapshot: &StoreSnapshot) -> ViewNode {
    let has_branches = !snapshot.branches.branches.is_empty();
    let header = if has_branches {
        "Branches Panel".to_string()
    } else {
        "Branches Panel (empty)".to_string()
    };
    let compare_line = match (
        snapshot.compare.base_ref.as_deref(),
        snapshot.compare.head_ref.as_deref(),
    ) {
        (Some(base_ref), Some(head_ref)) => Some(format!("Compare: {base_ref} -> {head_ref}")),
        _ => None,
    };

    let mut children = vec![
        ViewNode::Text { value: header },
        ViewNode::BranchList {
            title: "branches".to_string(),
        },
        ViewNode::TagList {
            title: "tags".to_string(),
        },
    ];
    if let Some(line) = compare_line {
        children.push(ViewNode::Text { value: line });
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
                render_into(child, snapshot, level, out);
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
            out.push_str(&format!("{indent}{title}: {}\n", items.join(", ")));
        }
        ViewNode::HistoryList { title } => {
            let selected = snapshot.selection.selected_commit_oid.as_deref();
            let list = snapshot
                .history
                .commits
                .iter()
                .map(|commit| {
                    let short = commit.oid.chars().take(7).collect::<String>();
                    let marker = if Some(commit.oid.as_str()) == selected {
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
        } => {
            let state = if *enabled_when { "enabled" } else { "disabled" };
            out.push_str(&format!("{indent}[{label}] {state} -> {on_action}\n"));
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
            journal: state_store::OperationJournalState::default(),
            active_view: None,
            plugins: Vec::new(),
            version: 1,
        };

        let panel = build_status_panel(&snapshot);
        let rendered = render(&panel, &snapshot);
        assert!(rendered.contains("Status Panel"));
        assert!(rendered.contains("[Commit] enabled"));
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
                    },
                    state_store::OperationJournalEntry {
                        id: 2,
                        job_id: None,
                        op: "commit.create".to_string(),
                        status: state_store::JournalStatus::Failed,
                        started_at_ms: 2,
                        finished_at_ms: Some(3),
                        error: Some("nothing to commit".to_string()),
                    },
                ],
            },
            active_view: None,
            plugins: Vec::new(),
            version: 1,
        };

        let panel = build_status_panel(&snapshot);
        let rendered = render(&panel, &snapshot);
        assert!(rendered.contains("Loading: status.refresh"));
        assert!(rendered.contains("Operation error: commit.create: nothing to commit"));
    }
}
