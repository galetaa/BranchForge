use std::collections::HashMap;

use plugin_api::RepoSnapshot;

pub type StoreVersion = u64;

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct StatusSnapshot {
    pub staged: Vec<String>,
    pub unstaged: Vec<String>,
    pub untracked: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct SelectionState {
    pub selected_paths: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct StoreSnapshot {
    pub repo: Option<RepoSnapshot>,
    pub status: StatusSnapshot,
    pub selection: SelectionState,
    pub version: StoreVersion,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum StateEvent {
    RepoOpened,
    Updated { version: StoreVersion },
}

#[derive(Debug, Default)]
pub struct StateStore {
    snapshot: StoreSnapshot,
    subscribers: HashMap<u64, Vec<StateEvent>>,
    next_subscriber_id: u64,
}

impl StateStore {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn snapshot(&self) -> &StoreSnapshot {
        &self.snapshot
    }

    pub fn repo(&self) -> Option<&RepoSnapshot> {
        self.snapshot.repo.as_ref()
    }

    pub fn update_repo(&mut self, repo: RepoSnapshot) {
        self.snapshot.repo = Some(repo);
        self.publish_event(StateEvent::RepoOpened);
        self.bump_version();
    }

    pub fn update_status(&mut self, status: StatusSnapshot) {
        self.snapshot.status = status;
        self.bump_version();
    }

    pub fn update_selection(&mut self, selection: SelectionState) {
        self.snapshot.selection = selection;
        self.bump_version();
    }

    pub fn subscribe(&mut self) -> u64 {
        let id = self.next_subscriber_id;
        self.next_subscriber_id += 1;
        self.subscribers.insert(id, Vec::new());
        id
    }

    pub fn poll_events(&mut self, subscriber_id: u64) -> Vec<StateEvent> {
        if let Some(queue) = self.subscribers.get_mut(&subscriber_id) {
            let events = queue.clone();
            queue.clear();
            events
        } else {
            Vec::new()
        }
    }

    fn bump_version(&mut self) {
        self.snapshot.version += 1;
        self.publish_event(StateEvent::Updated {
            version: self.snapshot.version,
        });
    }

    fn publish_event(&mut self, event: StateEvent) {
        for queue in self.subscribers.values_mut() {
            queue.push(event.clone());
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn stores_repo_snapshot() {
        let mut store = StateStore::new();
        let sub_id = store.subscribe();
        store.update_repo(RepoSnapshot {
            root: "./demo".to_string(),
            head: Some("main".to_string()),
        });

        let head = store.repo().and_then(|repo| repo.head.as_deref());
        assert_eq!(head, Some("main"));
        assert_eq!(store.snapshot().version, 1);

        let events = store.poll_events(sub_id);
        assert_eq!(events.len(), 2);
        assert!(matches!(events.first(), Some(StateEvent::RepoOpened)));
        assert!(matches!(
            events.get(1),
            Some(StateEvent::Updated { version }) if *version == 1
        ));
    }

    #[test]
    fn updates_typed_status_and_selection() {
        let mut store = StateStore::new();

        store.update_status(StatusSnapshot {
            staged: vec!["src/lib.rs".to_string()],
            unstaged: vec!["README.md".to_string()],
            untracked: vec!["notes.txt".to_string()],
        });
        store.update_selection(SelectionState {
            selected_paths: vec!["README.md".to_string()],
        });

        assert_eq!(store.snapshot().status.staged.len(), 1);
        assert_eq!(store.snapshot().selection.selected_paths.len(), 1);
        assert_eq!(store.snapshot().version, 2);
    }

    #[test]
    fn notifies_subscribers_on_updates() {
        let mut store = StateStore::new();
        let sub_id = store.subscribe();

        store.update_status(StatusSnapshot {
            staged: vec!["src/main.rs".to_string()],
            unstaged: Vec::new(),
            untracked: Vec::new(),
        });

        let events = store.poll_events(sub_id);
        assert_eq!(events.len(), 1);
        assert!(matches!(
            events.first(),
            Some(StateEvent::Updated { version }) if *version == 1
        ));
    }

    #[test]
    fn multiple_subscribers_receive_events_in_order() {
        let mut store = StateStore::new();
        let first = store.subscribe();
        let second = store.subscribe();

        store.update_repo(RepoSnapshot {
            root: "./demo".to_string(),
            head: Some("main".to_string()),
        });
        store.update_status(StatusSnapshot {
            staged: Vec::new(),
            unstaged: vec!["README.md".to_string()],
            untracked: Vec::new(),
        });

        let first_events = store.poll_events(first);
        let second_events = store.poll_events(second);
        assert_eq!(first_events, second_events);
        assert_eq!(first_events.len(), 3);
        assert!(matches!(first_events.first(), Some(StateEvent::RepoOpened)));
        assert!(matches!(
            first_events.get(1),
            Some(StateEvent::Updated { version }) if *version == 1
        ));
        assert!(matches!(
            first_events.get(2),
            Some(StateEvent::Updated { version }) if *version == 2
        ));

        assert!(store.poll_events(first).is_empty());
        assert!(store.poll_events(second).is_empty());
    }

    #[test]
    fn poll_events_returns_empty_for_unknown_subscriber() {
        let mut store = StateStore::new();
        store.update_status(StatusSnapshot {
            staged: vec!["src/main.rs".to_string()],
            unstaged: Vec::new(),
            untracked: Vec::new(),
        });

        let events = store.poll_events(999);
        assert!(events.is_empty());
    }
}
