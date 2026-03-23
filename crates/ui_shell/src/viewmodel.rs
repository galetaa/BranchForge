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
    Button {
        label: String,
        on_action: String,
        enabled_when: bool,
    },
}

pub fn build_status_panel(snapshot: &StoreSnapshot) -> ViewNode {
    ViewNode::Container {
        children: vec![
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
            ViewNode::Button {
                label: "Commit".to_string(),
                on_action: "commit.create".to_string(),
                enabled_when: !snapshot.status.staged.is_empty(),
            },
        ],
    }
}

pub fn build_history_panel(snapshot: &StoreSnapshot) -> ViewNode {
    let has_commits = !snapshot.history.commits.is_empty();
    let header = if has_commits {
        "History Panel".to_string()
    } else {
        "History Panel (empty)".to_string()
    };

    ViewNode::Container {
        children: vec![
            ViewNode::Text { value: header },
            ViewNode::HistoryList {
                title: "commits".to_string(),
            },
            ViewNode::CommitDetails,
        ],
    }
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
            let list = snapshot
                .history
                .commits
                .iter()
                .map(|commit| {
                    let short = commit.oid.chars().take(7).collect::<String>();
                    format!("{short} {}", commit.summary)
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
            active_view: None,
            plugins: Vec::new(),
            version: 1,
        };

        let panel = build_status_panel(&snapshot);
        let rendered = render(&panel, &snapshot);
        assert!(rendered.contains("Status Panel"));
        assert!(rendered.contains("[Commit] enabled"));
    }
}
