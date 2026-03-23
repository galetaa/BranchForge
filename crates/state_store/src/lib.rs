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
    pub selected_commit_oid: Option<String>,
    pub selected_branch: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CommitSummary {
    pub oid: String,
    pub author: String,
    pub time: String,
    pub summary: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CommitDetails {
    pub oid: String,
    pub author: String,
    pub time: String,
    pub message: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HistoryCursor {
    pub offset: usize,
    pub page_size: usize,
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct HistoryState {
    pub commits: Vec<CommitSummary>,
    pub next_cursor: Option<HistoryCursor>,
    pub filter_author: Option<String>,
    pub filter_text: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DiffSource {
    Worktree { paths: Vec<String> },
    Commit { oid: String },
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct DiffState {
    pub source: Option<DiffSource>,
    pub content: Option<String>,
    pub loading: bool,
    pub error: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BranchInfo {
    pub name: String,
    pub is_current: bool,
    pub upstream: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct BranchesState {
    pub branches: Vec<BranchInfo>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TagInfo {
    pub name: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct TagsState {
    pub tags: Vec<TagInfo>,
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct CommitMessageState {
    pub draft: String,
    pub error: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum JournalStatus {
    Started,
    Succeeded,
    Failed,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OperationJournalEntry {
    pub id: u64,
    pub job_id: Option<u64>,
    pub op: String,
    pub status: JournalStatus,
    pub started_at_ms: u64,
    pub finished_at_ms: Option<u64>,
    pub error: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct OperationJournalState {
    pub entries: Vec<OperationJournalEntry>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PluginHealth {
    Ready,
    Unavailable { message: String },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PluginStatus {
    pub plugin_id: String,
    pub health: PluginHealth,
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct StoreSnapshot {
    pub repo: Option<RepoSnapshot>,
    pub status: StatusSnapshot,
    pub selection: SelectionState,
    pub history: HistoryState,
    pub commit_cache: HashMap<String, CommitDetails>,
    pub diff: DiffState,
    pub branches: BranchesState,
    pub tags: TagsState,
    pub commit_message: CommitMessageState,
    pub journal: OperationJournalState,
    pub active_view: Option<String>,
    pub plugins: Vec<PluginStatus>,
    pub version: StoreVersion,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum StateEvent {
    RepoOpened,
    Updated { version: StoreVersion },
}

#[derive(Debug)]
pub struct StateStore {
    snapshot: StoreSnapshot,
    subscribers: HashMap<u64, Vec<StateEvent>>,
    next_subscriber_id: u64,
    next_journal_id: u64,
}

impl Default for StateStore {
    fn default() -> Self {
        Self {
            snapshot: StoreSnapshot::default(),
            subscribers: HashMap::new(),
            next_subscriber_id: 1,
            next_journal_id: 1,
        }
    }
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

    pub fn update_selected_paths(&mut self, paths: Vec<String>) {
        self.snapshot.selection.selected_paths = paths;
        self.snapshot.selection.selected_commit_oid = None;
        self.snapshot.selection.selected_branch = None;
        self.bump_version();
    }

    pub fn update_selected_commit(&mut self, oid: Option<String>) {
        self.snapshot.selection.selected_commit_oid = oid;
        self.snapshot.selection.selected_paths.clear();
        self.snapshot.selection.selected_branch = None;
        self.bump_version();
    }

    pub fn update_selected_branch(&mut self, name: Option<String>) {
        self.snapshot.selection.selected_branch = name;
        self.snapshot.selection.selected_commit_oid = None;
        self.snapshot.selection.selected_paths.clear();
        self.bump_version();
    }

    pub fn update_history_page(
        &mut self,
        commits: Vec<CommitSummary>,
        next_cursor: Option<HistoryCursor>,
        append: bool,
        filter_author: Option<String>,
        filter_text: Option<String>,
    ) {
        if append {
            self.snapshot.history.commits.extend(commits);
        } else {
            self.snapshot.history.commits = commits;
        }
        self.snapshot.history.next_cursor = next_cursor;
        self.snapshot.history.filter_author = filter_author;
        self.snapshot.history.filter_text = filter_text;
        self.bump_version();
    }

    pub fn clear_history(&mut self) {
        self.snapshot.history = HistoryState::default();
        self.snapshot.commit_cache.clear();
        self.bump_version();
    }

    pub fn append_journal_entry(
        &mut self,
        job_id: Option<u64>,
        op: String,
        started_at_ms: u64,
    ) -> u64 {
        let entry_id = self.next_journal_id;
        self.next_journal_id += 1;
        self.snapshot.journal.entries.push(OperationJournalEntry {
            id: entry_id,
            job_id,
            op,
            status: JournalStatus::Started,
            started_at_ms,
            finished_at_ms: None,
            error: None,
        });
        self.bump_version();
        entry_id
    }

    pub fn finish_journal_entry(
        &mut self,
        entry_id: u64,
        status: JournalStatus,
        finished_at_ms: u64,
        error: Option<String>,
    ) {
        if let Some(entry) = self
            .snapshot
            .journal
            .entries
            .iter_mut()
            .find(|entry| entry.id == entry_id)
        {
            entry.status = status;
            entry.finished_at_ms = Some(finished_at_ms);
            entry.error = error;
        }
        self.bump_version();
    }

    pub fn clear_journal(&mut self) {
        self.snapshot.journal = OperationJournalState::default();
        self.bump_version();
    }

    pub fn update_commit_details(&mut self, details: CommitDetails) {
        self.snapshot
            .commit_cache
            .insert(details.oid.clone(), details);
        self.bump_version();
    }

    pub fn commit_details(&self, oid: &str) -> Option<&CommitDetails> {
        self.snapshot.commit_cache.get(oid)
    }

    pub fn update_diff(&mut self, diff: DiffState) {
        self.snapshot.diff = diff;
        self.bump_version();
    }

    pub fn update_branches(&mut self, branches: Vec<BranchInfo>) {
        self.snapshot.branches.branches = branches;
        self.bump_version();
    }

    pub fn update_tags(&mut self, tags: Vec<TagInfo>) {
        self.snapshot.tags.tags = tags;
        self.bump_version();
    }

    pub fn update_commit_message(&mut self, draft: String, error: Option<String>) {
        self.snapshot.commit_message.draft = draft;
        self.snapshot.commit_message.error = error;
        self.bump_version();
    }

    pub fn set_active_view(&mut self, view: Option<String>) {
        self.snapshot.active_view = view;
        self.bump_version();
    }

    pub fn update_plugin_status(&mut self, plugin_id: &str, health: PluginHealth) {
        if let Some(existing) = self
            .snapshot
            .plugins
            .iter_mut()
            .find(|status| status.plugin_id == plugin_id)
        {
            existing.health = health;
        } else {
            self.snapshot.plugins.push(PluginStatus {
                plugin_id: plugin_id.to_string(),
                health,
            });
        }
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
            selected_commit_oid: None,
            selected_branch: None,
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
    fn update_plugin_status_inserts_and_updates() {
        let mut store = StateStore::new();
        store.update_plugin_status("status", PluginHealth::Ready);
        assert_eq!(store.snapshot().plugins.len(), 1);
        assert!(matches!(
            store.snapshot().plugins[0].health,
            PluginHealth::Ready
        ));

        store.update_plugin_status(
            "status",
            PluginHealth::Unavailable {
                message: "crashed".to_string(),
            },
        );
        assert_eq!(store.snapshot().plugins.len(), 1);
        assert!(matches!(
            store.snapshot().plugins[0].health,
            PluginHealth::Unavailable { ref message } if message == "crashed"
        ));
        assert_eq!(store.snapshot().version, 2);
    }

    #[test]
    fn updates_commit_selection_clears_paths() {
        let mut store = StateStore::new();
        store.update_selected_paths(vec!["README.md".to_string()]);
        assert_eq!(store.snapshot().selection.selected_paths.len(), 1);
        store.update_selected_commit(Some("abc123".to_string()));
        assert!(store.snapshot().selection.selected_paths.is_empty());
        assert_eq!(
            store.snapshot().selection.selected_commit_oid.as_deref(),
            Some("abc123")
        );
    }

    #[test]
    fn updates_branch_selection_clears_commit() {
        let mut store = StateStore::new();
        store.update_selected_commit(Some("abc123".to_string()));
        store.update_selected_branch(Some("feature".to_string()));
        assert!(store.snapshot().selection.selected_commit_oid.is_none());
        assert_eq!(
            store.snapshot().selection.selected_branch.as_deref(),
            Some("feature")
        );
    }

    #[test]
    fn commit_message_update_tracks_error() {
        let mut store = StateStore::new();
        store.update_commit_message("feat: msg".to_string(), None);
        assert_eq!(store.snapshot().commit_message.draft, "feat: msg");
        assert!(store.snapshot().commit_message.error.is_none());

        store.update_commit_message("".to_string(), Some("empty".to_string()));
        assert_eq!(store.snapshot().commit_message.draft, "");
        assert_eq!(
            store.snapshot().commit_message.error.as_deref(),
            Some("empty")
        );
    }

    #[test]
    fn journal_entry_lifecycle_updates_status() {
        let mut store = StateStore::new();
        let entry_id = store.append_journal_entry(Some(42), "commit.create".to_string(), 100);
        assert_eq!(store.snapshot().journal.entries.len(), 1);
        assert!(matches!(
            store.snapshot().journal.entries[0].status,
            JournalStatus::Started
        ));

        store.finish_journal_entry(entry_id, JournalStatus::Succeeded, 200, None);
        let entry = &store.snapshot().journal.entries[0];
        assert_eq!(entry.job_id, Some(42));
        assert_eq!(entry.finished_at_ms, Some(200));
        assert!(matches!(entry.status, JournalStatus::Succeeded));
    }

    #[test]
    fn history_page_appends_and_tracks_cursor() {
        let mut store = StateStore::new();
        store.update_history_page(
            vec![CommitSummary {
                oid: "a".to_string(),
                author: "Dev".to_string(),
                time: "now".to_string(),
                summary: "first".to_string(),
            }],
            Some(HistoryCursor {
                offset: 1,
                page_size: 1,
            }),
            false,
            None,
            None,
        );
        store.update_history_page(
            vec![CommitSummary {
                oid: "b".to_string(),
                author: "Dev".to_string(),
                time: "now".to_string(),
                summary: "second".to_string(),
            }],
            None,
            true,
            None,
            None,
        );
        assert_eq!(store.snapshot().history.commits.len(), 2);
        assert!(store.snapshot().history.next_cursor.is_none());
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
