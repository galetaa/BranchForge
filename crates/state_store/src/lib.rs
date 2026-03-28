use std::collections::HashMap;
use std::path::Path;

use plugin_api::{ConflictState, RepoSnapshot};
use serde::{Deserialize, Serialize};

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
    pub selected_plugin_id: Option<String>,
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
    pub loading: bool,
    pub error: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BlameLine {
    pub line_no: usize,
    pub oid: String,
    pub author: String,
    pub content: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct BlameState {
    pub path: Option<String>,
    pub rev: Option<String>,
    pub lines: Vec<BlameLine>,
    pub loading: bool,
    pub error: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DiffSource {
    Worktree { paths: Vec<String> },
    Index { paths: Vec<String> },
    Commit { oid: String },
    Compare { base: String, head: String },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DiffLoadRequest {
    pub source: DiffSource,
    pub chunk_size: usize,
    pub cursor: Option<usize>,
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct DiffDescriptor {
    pub total_bytes: usize,
    pub chunk_size: usize,
    pub loaded_chunks: usize,
    pub truncated: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct DiffChunk {
    pub index: usize,
    pub content: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct DiffState {
    pub source: Option<DiffSource>,
    pub descriptor: Option<DiffDescriptor>,
    pub load_request: Option<DiffLoadRequest>,
    pub chunks: Vec<DiffChunk>,
    pub content: Option<String>,
    pub hunks: Vec<DiffHunk>,
    pub loading: bool,
    pub error: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct CompareState {
    pub base_ref: Option<String>,
    pub head_ref: Option<String>,
    pub ahead: usize,
    pub behind: usize,
    pub commits: Vec<CommitSummary>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DiffHunk {
    pub file_path: String,
    pub hunk_index: usize,
    pub header: String,
    pub lines: Vec<String>,
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
pub enum RebaseEntryAction {
    Pick,
    Reword,
    Edit,
    Squash,
    Fixup,
    Drop,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RebasePlanEntry {
    pub oid: String,
    pub summary: String,
    pub action: RebaseEntryAction,
    pub warnings: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct RebasePlan {
    pub base_ref: String,
    pub base_oid: Option<String>,
    pub entries: Vec<RebasePlanEntry>,
    pub affected_commit_count: usize,
    pub rewrite_types: Vec<String>,
    pub published_history_warning: Option<String>,
    pub autosquash_aware: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct RebaseSessionSnapshot {
    pub active: bool,
    pub repo_root: Option<String>,
    pub base_ref: Option<String>,
    pub current_step: Option<usize>,
    pub total_steps: Option<usize>,
    pub blocking_conflict: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct RebaseState {
    pub plan: Option<RebasePlan>,
    pub session: Option<RebaseSessionSnapshot>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum JournalStatus {
    Started,
    Succeeded,
    Failed,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum OperationSessionKind {
    AdvancedBranchOperation,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum OperationSessionState {
    Running,
    Succeeded,
    Failed,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RefSnapshotSummary {
    pub head: Option<String>,
    pub branch_count: usize,
    pub tag_count: usize,
    pub conflict_state: Option<ConflictState>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct OperationJournalEntry {
    pub id: u64,
    pub job_id: Option<u64>,
    pub op: String,
    pub status: JournalStatus,
    pub started_at_ms: u64,
    pub finished_at_ms: Option<u64>,
    pub error: Option<String>,
    #[serde(default)]
    pub session_id: Option<u64>,
    #[serde(default)]
    pub session_kind: Option<OperationSessionKind>,
    #[serde(default)]
    pub session_state: Option<OperationSessionState>,
    #[serde(default)]
    pub pre_refs: Option<RefSnapshotSummary>,
    #[serde(default)]
    pub post_refs: Option<RefSnapshotSummary>,
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
pub struct InstalledPluginRecord {
    pub plugin_id: String,
    pub version: String,
    pub protocol_version: String,
    pub enabled: bool,
    pub description: Option<String>,
    pub permissions: Vec<String>,
    pub install_dir: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct StoreSnapshot {
    pub repo: Option<RepoSnapshot>,
    pub status: StatusSnapshot,
    pub selection: SelectionState,
    pub history: HistoryState,
    pub blame: BlameState,
    pub commit_cache: HashMap<String, CommitDetails>,
    pub diff: DiffState,
    pub compare: CompareState,
    pub branches: BranchesState,
    pub tags: TagsState,
    pub commit_message: CommitMessageState,
    pub rebase: RebaseState,
    pub journal: OperationJournalState,
    pub active_view: Option<String>,
    pub plugins: Vec<PluginStatus>,
    pub installed_plugins: Vec<InstalledPluginRecord>,
    pub version: StoreVersion,
}

impl StoreSnapshot {
    pub fn history_can_load_more(&self) -> bool {
        self.history.next_cursor.is_some()
    }

    pub fn history_has_selection(&self) -> bool {
        self.selection.selected_commit_oid.is_some()
    }
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
    next_session_id: u64,
}

const JOURNAL_RETENTION_LIMIT: usize = 200;
const COMMIT_CACHE_LIMIT: usize = 256;

impl Default for StateStore {
    fn default() -> Self {
        Self {
            snapshot: StoreSnapshot::default(),
            subscribers: HashMap::new(),
            next_subscriber_id: 1,
            next_journal_id: 1,
            next_session_id: 1,
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

    pub fn update_selected_plugin(&mut self, plugin_id: Option<String>) {
        self.snapshot.selection.selected_plugin_id = plugin_id;
        self.bump_version();
    }

    pub fn clear_repo_selection_preserving_plugin(&mut self) {
        self.snapshot.selection.selected_paths.clear();
        self.snapshot.selection.selected_commit_oid = None;
        self.snapshot.selection.selected_branch = None;
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
        self.snapshot.history.loading = false;
        self.snapshot.history.error = None;
        self.bump_version();
    }

    pub fn clear_history(&mut self) {
        self.snapshot.history = HistoryState::default();
        self.snapshot.blame = BlameState::default();
        self.snapshot.commit_cache.clear();
        self.bump_version();
    }

    pub fn set_history_loading(&mut self, loading: bool) {
        self.snapshot.history.loading = loading;
        if loading {
            self.snapshot.history.error = None;
        }
        self.bump_version();
    }

    pub fn set_history_error(&mut self, message: String) {
        self.snapshot.history.loading = false;
        self.snapshot.history.error = Some(message);
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
            session_id: None,
            session_kind: None,
            session_state: None,
            pre_refs: None,
            post_refs: None,
        });
        self.enforce_journal_retention();
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

    pub fn allocate_session_id(&mut self) -> u64 {
        let id = self.next_session_id;
        self.next_session_id += 1;
        id
    }

    pub fn set_journal_session(
        &mut self,
        entry_id: u64,
        session_id: u64,
        session_kind: OperationSessionKind,
        session_state: OperationSessionState,
    ) {
        if let Some(entry) = self
            .snapshot
            .journal
            .entries
            .iter_mut()
            .find(|entry| entry.id == entry_id)
        {
            entry.session_id = Some(session_id);
            entry.session_kind = Some(session_kind);
            entry.session_state = Some(session_state);
        }
        self.bump_version();
    }

    pub fn set_journal_session_state(
        &mut self,
        entry_id: u64,
        session_state: OperationSessionState,
    ) {
        if let Some(entry) = self
            .snapshot
            .journal
            .entries
            .iter_mut()
            .find(|entry| entry.id == entry_id)
        {
            entry.session_state = Some(session_state);
        }
        self.bump_version();
    }

    pub fn set_journal_pre_refs(&mut self, entry_id: u64, pre_refs: RefSnapshotSummary) {
        if let Some(entry) = self
            .snapshot
            .journal
            .entries
            .iter_mut()
            .find(|entry| entry.id == entry_id)
        {
            entry.pre_refs = Some(pre_refs);
        }
        self.bump_version();
    }

    pub fn set_journal_post_refs(&mut self, entry_id: u64, post_refs: RefSnapshotSummary) {
        if let Some(entry) = self
            .snapshot
            .journal
            .entries
            .iter_mut()
            .find(|entry| entry.id == entry_id)
        {
            entry.post_refs = Some(post_refs);
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
        while self.snapshot.commit_cache.len() > COMMIT_CACHE_LIMIT {
            if let Some(key) = self.snapshot.commit_cache.keys().next().cloned() {
                self.snapshot.commit_cache.remove(&key);
            } else {
                break;
            }
        }
        self.bump_version();
    }

    pub fn commit_details(&self, oid: &str) -> Option<&CommitDetails> {
        self.snapshot.commit_cache.get(oid)
    }

    pub fn update_blame(&mut self, path: String, rev: Option<String>, lines: Vec<BlameLine>) {
        self.snapshot.blame = BlameState {
            path: Some(path),
            rev,
            lines,
            loading: false,
            error: None,
        };
        self.bump_version();
    }

    pub fn clear_blame(&mut self) {
        self.snapshot.blame = BlameState::default();
        self.bump_version();
    }

    pub fn update_diff(&mut self, diff: DiffState) {
        self.snapshot.diff = diff;
        self.bump_version();
    }

    pub fn update_compare(&mut self, base_ref: String, head_ref: String) {
        self.snapshot.compare = CompareState {
            base_ref: Some(base_ref),
            head_ref: Some(head_ref),
            ahead: 0,
            behind: 0,
            commits: Vec::new(),
        };
        self.bump_version();
    }

    pub fn update_compare_summary(
        &mut self,
        base_ref: String,
        head_ref: String,
        ahead: usize,
        behind: usize,
        commits: Vec<CommitSummary>,
    ) {
        self.snapshot.compare = CompareState {
            base_ref: Some(base_ref),
            head_ref: Some(head_ref),
            ahead,
            behind,
            commits,
        };
        self.bump_version();
    }

    pub fn clear_compare(&mut self) {
        self.snapshot.compare = CompareState::default();
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

    pub fn update_rebase_plan(&mut self, plan: RebasePlan) {
        self.snapshot.rebase.plan = Some(plan);
        self.bump_version();
    }

    pub fn clear_rebase_plan(&mut self) {
        self.snapshot.rebase.plan = None;
        self.bump_version();
    }

    pub fn update_rebase_session(&mut self, session: RebaseSessionSnapshot) {
        self.snapshot.rebase.session = Some(session);
        self.bump_version();
    }

    pub fn clear_rebase_session(&mut self) {
        self.snapshot.rebase.session = None;
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

    pub fn update_installed_plugins(&mut self, plugins: Vec<InstalledPluginRecord>) {
        self.snapshot.installed_plugins = plugins;
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

    fn enforce_journal_retention(&mut self) {
        let len = self.snapshot.journal.entries.len();
        if len > JOURNAL_RETENTION_LIMIT {
            let to_drop = len - JOURNAL_RETENTION_LIMIT;
            self.snapshot.journal.entries.drain(0..to_drop);
        }
    }

    pub fn persist_journal(&self, path: &Path) -> Result<(), String> {
        let payload = serde_json::to_string_pretty(&self.snapshot.journal.entries)
            .map_err(|err| err.to_string())?;
        std::fs::write(path, payload).map_err(|err| err.to_string())
    }

    pub fn restore_journal(&mut self, path: &Path) -> Result<(), String> {
        if !path.exists() {
            return Ok(());
        }
        let payload = std::fs::read_to_string(path).map_err(|err| err.to_string())?;
        let entries: Vec<OperationJournalEntry> =
            serde_json::from_str(&payload).map_err(|err| err.to_string())?;
        self.snapshot.journal.entries = entries;
        self.enforce_journal_retention();
        self.next_journal_id = self
            .snapshot
            .journal
            .entries
            .last()
            .map(|entry| entry.id + 1)
            .unwrap_or(1);
        self.next_session_id = self
            .snapshot
            .journal
            .entries
            .iter()
            .filter_map(|entry| entry.session_id)
            .max()
            .map(|id| id + 1)
            .unwrap_or(1);
        self.bump_version();
        Ok(())
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
            conflict_state: None,
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
            selected_plugin_id: None,
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
    fn update_installed_plugins_replaces_inventory() {
        let mut store = StateStore::new();
        store.update_installed_plugins(vec![InstalledPluginRecord {
            plugin_id: "status".to_string(),
            version: "0.1.0".to_string(),
            protocol_version: "1.0".to_string(),
            enabled: true,
            description: Some("status plugin".to_string()),
            permissions: vec!["read_state".to_string()],
            install_dir: "/tmp/plugins/status".to_string(),
        }]);
        assert_eq!(store.snapshot().installed_plugins.len(), 1);
        assert_eq!(store.snapshot().installed_plugins[0].plugin_id, "status");

        store.update_installed_plugins(vec![InstalledPluginRecord {
            plugin_id: "history".to_string(),
            version: "0.2.0".to_string(),
            protocol_version: "1.0".to_string(),
            enabled: false,
            description: None,
            permissions: vec!["read_state".to_string(), "read_git".to_string()],
            install_dir: "/tmp/plugins/history".to_string(),
        }]);

        assert_eq!(store.snapshot().installed_plugins.len(), 1);
        assert_eq!(store.snapshot().installed_plugins[0].plugin_id, "history");
        assert!(!store.snapshot().installed_plugins[0].enabled);
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
    fn updates_plugin_selection_without_clearing_repo_selection() {
        let mut store = StateStore::new();
        store.update_selected_branch(Some("feature".to_string()));
        store.update_selected_plugin(Some("status".to_string()));
        assert_eq!(
            store.snapshot().selection.selected_branch.as_deref(),
            Some("feature")
        );
        assert_eq!(
            store.snapshot().selection.selected_plugin_id.as_deref(),
            Some("status")
        );
    }

    #[test]
    fn clears_repo_selection_but_keeps_plugin_selection() {
        let mut store = StateStore::new();
        store.update_selected_paths(vec!["README.md".to_string()]);
        store.update_selected_plugin(Some("status".to_string()));
        store.clear_repo_selection_preserving_plugin();
        assert!(store.snapshot().selection.selected_paths.is_empty());
        assert!(store.snapshot().selection.selected_commit_oid.is_none());
        assert!(store.snapshot().selection.selected_branch.is_none());
        assert_eq!(
            store.snapshot().selection.selected_plugin_id.as_deref(),
            Some("status")
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
    fn journal_entry_tracks_session_and_ref_snapshots() {
        let mut store = StateStore::new();
        let entry_id = store.append_journal_entry(None, "merge.execute".to_string(), 10);
        let session_id = store.allocate_session_id();
        store.set_journal_session(
            entry_id,
            session_id,
            OperationSessionKind::AdvancedBranchOperation,
            OperationSessionState::Running,
        );
        store.set_journal_pre_refs(
            entry_id,
            RefSnapshotSummary {
                head: Some("main".to_string()),
                branch_count: 2,
                tag_count: 1,
                conflict_state: None,
            },
        );
        store.set_journal_session_state(entry_id, OperationSessionState::Succeeded);
        store.set_journal_post_refs(
            entry_id,
            RefSnapshotSummary {
                head: Some("main".to_string()),
                branch_count: 2,
                tag_count: 1,
                conflict_state: None,
            },
        );

        let entry = store
            .snapshot()
            .journal
            .entries
            .iter()
            .find(|entry| entry.id == entry_id)
            .cloned();
        assert!(entry.is_some());
        if let Some(entry) = entry {
            assert_eq!(entry.session_id, Some(session_id));
            assert!(matches!(
                entry.session_kind,
                Some(OperationSessionKind::AdvancedBranchOperation)
            ));
            assert!(matches!(
                entry.session_state,
                Some(OperationSessionState::Succeeded)
            ));
            assert!(entry.pre_refs.is_some());
            assert!(entry.post_refs.is_some());
        }
    }

    #[test]
    fn journal_retention_keeps_recent_entries() {
        let mut store = StateStore::new();
        for idx in 0..(JOURNAL_RETENTION_LIMIT + 10) {
            let _ = store.append_journal_entry(None, format!("op.{idx}"), idx as u64);
        }
        assert_eq!(
            store.snapshot().journal.entries.len(),
            JOURNAL_RETENTION_LIMIT
        );
        let first = store
            .snapshot()
            .journal
            .entries
            .first()
            .map(|entry| entry.op.clone())
            .unwrap_or_default();
        assert_eq!(first, "op.10");
    }

    #[test]
    fn journal_persists_and_restores_from_file() {
        let mut store = StateStore::new();
        let entry_id = store.append_journal_entry(Some(7), "tag.delete".to_string(), 100);
        store.finish_journal_entry(
            entry_id,
            JournalStatus::Failed,
            200,
            Some("boom".to_string()),
        );

        let path = std::env::temp_dir().join("branchforge-journal-state-store-test.json");
        let write = store.persist_journal(&path);
        assert!(write.is_ok());

        let mut restored = StateStore::new();
        let read = restored.restore_journal(&path);
        assert!(read.is_ok());
        assert_eq!(restored.snapshot().journal.entries.len(), 1);
        assert_eq!(restored.snapshot().journal.entries[0].op, "tag.delete");
        let _ = std::fs::remove_file(path);
    }

    #[test]
    fn updates_rebase_plan_and_session_state() {
        let mut store = StateStore::new();
        store.update_rebase_plan(RebasePlan {
            base_ref: "main".to_string(),
            base_oid: Some("abc".to_string()),
            entries: vec![RebasePlanEntry {
                oid: "def".to_string(),
                summary: "feat: commit".to_string(),
                action: RebaseEntryAction::Pick,
                warnings: Vec::new(),
            }],
            affected_commit_count: 1,
            rewrite_types: vec!["pick".to_string()],
            published_history_warning: None,
            autosquash_aware: false,
        });
        store.update_rebase_session(RebaseSessionSnapshot {
            active: true,
            repo_root: Some("/tmp/repo".to_string()),
            base_ref: Some("main".to_string()),
            current_step: Some(1),
            total_steps: Some(1),
            blocking_conflict: false,
        });

        assert!(store.snapshot().rebase.plan.is_some());
        assert!(store.snapshot().rebase.session.is_some());

        store.clear_rebase_plan();
        store.clear_rebase_session();
        assert!(store.snapshot().rebase.plan.is_none());
        assert!(store.snapshot().rebase.session.is_none());
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
    fn compare_state_tracks_refs() {
        let mut store = StateStore::new();
        store.update_compare("main".to_string(), "feature".to_string());
        assert_eq!(store.snapshot().compare.base_ref.as_deref(), Some("main"));
        assert_eq!(
            store.snapshot().compare.head_ref.as_deref(),
            Some("feature")
        );
        store.clear_compare();
        assert!(store.snapshot().compare.base_ref.is_none());
    }

    #[test]
    fn commit_cache_is_bounded() {
        let mut store = StateStore::new();
        for idx in 0..(COMMIT_CACHE_LIMIT + 20) {
            store.update_commit_details(CommitDetails {
                oid: format!("oid-{idx}"),
                author: "Dev".to_string(),
                time: "now".to_string(),
                message: format!("msg-{idx}"),
            });
        }
        assert!(store.snapshot().commit_cache.len() <= COMMIT_CACHE_LIMIT);
    }

    #[test]
    fn multiple_subscribers_receive_events_in_order() {
        let mut store = StateStore::new();
        let first = store.subscribe();
        let second = store.subscribe();

        store.update_repo(RepoSnapshot {
            root: "./demo".to_string(),
            head: Some("main".to_string()),
            conflict_state: None,
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

    #[test]
    fn blame_state_tracks_selected_file_lines() {
        let mut store = StateStore::new();
        store.update_blame(
            "src/lib.rs".to_string(),
            Some("HEAD".to_string()),
            vec![BlameLine {
                line_no: 1,
                oid: "abc123".to_string(),
                author: "Dev".to_string(),
                content: "fn demo() {}".to_string(),
            }],
        );

        assert_eq!(store.snapshot().blame.path.as_deref(), Some("src/lib.rs"));
        assert_eq!(store.snapshot().blame.rev.as_deref(), Some("HEAD"));
        assert_eq!(store.snapshot().blame.lines.len(), 1);

        store.clear_blame();
        assert!(store.snapshot().blame.path.is_none());
        assert!(store.snapshot().blame.lines.is_empty());
    }
}
