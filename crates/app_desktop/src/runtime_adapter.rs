use std::path::Path;
use std::sync::mpsc::{self, Receiver, Sender};
use std::sync::{Arc, Mutex};
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};

use app_host::{
    ConsoleRunnerConfig, HostActionCatalogItem, HostReflogEntry, HostRuntime, HostRuntimeError,
};
use graph_model::GraphCommit;
use state_store::{OperationPreview, StoreSnapshot};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DesktopRuntimeError {
    pub title: String,
    pub message: String,
    pub detail: Option<String>,
}

impl DesktopRuntimeError {
    fn system(message: impl Into<String>) -> Self {
        Self {
            title: "Desktop runtime unavailable".to_string(),
            message: message.into(),
            detail: None,
        }
    }
}

impl std::fmt::Display for DesktopRuntimeError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        if let Some(detail) = self.detail.as_deref() {
            write!(f, "{}: {} ({detail})", self.title, self.message)
        } else {
            write!(f, "{}: {}", self.title, self.message)
        }
    }
}

impl From<HostRuntimeError> for DesktopRuntimeError {
    fn from(value: HostRuntimeError) -> Self {
        Self {
            title: value.title,
            message: value.message,
            detail: value.detail,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct RuntimeAdapterState {
    pub snapshot: StoreSnapshot,
    pub action_catalog: Vec<HostActionCatalogItem>,
    pub graph_rows: Vec<GraphCommit>,
    pub reflog_entries: Vec<HostReflogEntry>,
    pub busy: bool,
    pub current_operation: Option<String>,
    pub last_message: Option<String>,
    pub last_error: Option<DesktopRuntimeError>,
}

impl RuntimeAdapterState {
    fn lock_failed() -> Self {
        Self {
            last_error: Some(DesktopRuntimeError::system("runtime state lock failed")),
            ..Self::default()
        }
    }
}

enum RuntimeCommand {
    SubmitLine {
        line: String,
        label: String,
    },
    Preview {
        action_id: String,
        args: Vec<String>,
        response: Sender<Result<OperationPreview, DesktopRuntimeError>>,
    },
    LoadReflog {
        reference: String,
        limit: usize,
        response: Sender<Result<Vec<HostReflogEntry>, DesktopRuntimeError>>,
    },
    Shutdown,
}

pub struct DesktopRuntimeAdapter {
    sender: Sender<RuntimeCommand>,
    state: Arc<Mutex<RuntimeAdapterState>>,
    worker: Option<JoinHandle<()>>,
}

impl DesktopRuntimeAdapter {
    pub fn from_current_env() -> Result<Self, String> {
        let mut config = ConsoleRunnerConfig::from_current_env()?;
        config.auto_render = false;
        Ok(Self::new(config))
    }

    pub fn new(config: ConsoleRunnerConfig) -> Self {
        let runtime = HostRuntime::new(config);
        let initial_state = RuntimeAdapterState {
            snapshot: runtime.snapshot(),
            action_catalog: runtime.action_catalog(),
            graph_rows: runtime.history_graph().unwrap_or_default(),
            last_message: Some("desktop runtime initialized".to_string()),
            ..RuntimeAdapterState::default()
        };
        let state = Arc::new(Mutex::new(initial_state));
        let (sender, receiver) = mpsc::channel();
        let worker_state = Arc::clone(&state);
        let worker = thread::spawn(move || worker_loop(runtime, receiver, worker_state));

        Self {
            sender,
            state,
            worker: Some(worker),
        }
    }

    pub fn state(&self) -> RuntimeAdapterState {
        match self.state.lock() {
            Ok(state) => state.clone(),
            Err(_) => RuntimeAdapterState::lock_failed(),
        }
    }

    pub fn open_repo(&self, path: impl AsRef<Path>) -> Result<(), DesktopRuntimeError> {
        let path = path.as_ref().to_string_lossy().to_string();
        self.submit_line(
            format!("open {}", quote_arg(&path)),
            format!("Open repository {path}"),
        )
    }

    pub fn switch_panel(&self, panel: &str) -> Result<(), DesktopRuntimeError> {
        self.submit_line(
            format!("panel {}", quote_arg(panel)),
            format!("Open {panel} panel"),
        )
    }

    pub fn refresh(&self) -> Result<(), DesktopRuntimeError> {
        self.submit_line("refresh".to_string(), "Refresh".to_string())
    }

    pub fn select_file(&self, path: &str) -> Result<(), DesktopRuntimeError> {
        self.submit_line(
            format!("select file {}", quote_arg(path)),
            format!("Select file {path}"),
        )
    }

    pub fn select_commit(&self, oid: &str) -> Result<(), DesktopRuntimeError> {
        self.submit_line(
            format!("select commit {}", quote_arg(oid)),
            format!("Select commit {oid}"),
        )
    }

    pub fn select_branch(&self, name: &str) -> Result<(), DesktopRuntimeError> {
        self.submit_line(
            format!("select branch {}", quote_arg(name)),
            format!("Select branch {name}"),
        )
    }

    pub fn execute_action(
        &self,
        action_id: &str,
        args: &[String],
        confirmed: bool,
    ) -> Result<(), DesktopRuntimeError> {
        let mut tokens = vec!["run".to_string()];
        if confirmed {
            tokens.push("--confirm".to_string());
        }
        tokens.push(quote_arg(action_id));
        tokens.extend(args.iter().map(|arg| quote_arg(arg)));
        self.submit_line(tokens.join(" "), format!("Run {action_id}"))
    }

    pub fn preview_action(
        &self,
        action_id: &str,
        args: &[String],
    ) -> Result<OperationPreview, DesktopRuntimeError> {
        let (response, receiver) = mpsc::channel();
        self.sender
            .send(RuntimeCommand::Preview {
                action_id: action_id.to_string(),
                args: args.to_vec(),
                response,
            })
            .map_err(|_| DesktopRuntimeError::system("runtime worker stopped"))?;
        receiver
            .recv()
            .map_err(|_| DesktopRuntimeError::system("runtime preview response dropped"))?
    }

    pub fn load_reflog(
        &self,
        reference: &str,
        limit: usize,
    ) -> Result<Vec<HostReflogEntry>, DesktopRuntimeError> {
        let (response, receiver) = mpsc::channel();
        self.sender
            .send(RuntimeCommand::LoadReflog {
                reference: reference.to_string(),
                limit,
                response,
            })
            .map_err(|_| DesktopRuntimeError::system("runtime worker stopped"))?;
        receiver
            .recv()
            .map_err(|_| DesktopRuntimeError::system("runtime reflog response dropped"))?
    }

    pub fn wait_for_idle(&self, timeout: Duration) -> bool {
        let started = Instant::now();
        loop {
            if !self.state().busy {
                return true;
            }
            if started.elapsed() >= timeout {
                return false;
            }
            thread::sleep(Duration::from_millis(10));
        }
    }

    fn submit_line(&self, line: String, label: String) -> Result<(), DesktopRuntimeError> {
        match self.state.lock() {
            Ok(mut state) => {
                state.busy = true;
                state.current_operation = Some(label.clone());
                state.last_error = None;
            }
            Err(_) => return Err(DesktopRuntimeError::system("runtime state lock failed")),
        }

        self.sender
            .send(RuntimeCommand::SubmitLine { line, label })
            .map_err(|_| DesktopRuntimeError::system("runtime worker stopped"))
    }
}

impl Drop for DesktopRuntimeAdapter {
    fn drop(&mut self) {
        let _ = self.sender.send(RuntimeCommand::Shutdown);
        if let Some(worker) = self.worker.take() {
            let _ = worker.join();
        }
    }
}

fn worker_loop(
    mut runtime: HostRuntime,
    receiver: Receiver<RuntimeCommand>,
    state: Arc<Mutex<RuntimeAdapterState>>,
) {
    while let Ok(command) = receiver.recv() {
        match command {
            RuntimeCommand::Shutdown => break,
            RuntimeCommand::Preview {
                action_id,
                args,
                response,
            } => {
                let result = runtime
                    .preview_action(&action_id, &args)
                    .map_err(DesktopRuntimeError::from);
                let _ = response.send(result);
            }
            RuntimeCommand::LoadReflog {
                reference,
                limit,
                response,
            } => {
                let result = runtime
                    .reflog_entries(&reference, limit)
                    .map_err(DesktopRuntimeError::from);
                if let Ok(entries) = result.as_ref()
                    && let Ok(mut state) = state.lock()
                {
                    state.reflog_entries = entries.clone();
                }
                let _ = response.send(result);
            }
            RuntimeCommand::SubmitLine { line, label } => {
                if let Ok(mut state) = state.lock() {
                    state.busy = true;
                    state.current_operation = Some(label.clone());
                    state.last_error = None;
                }

                let result = runtime.submit_line(&line);
                let snapshot = runtime.snapshot();
                let action_catalog = runtime.action_catalog();
                let graph_rows = runtime.history_graph().unwrap_or_default();

                if let Ok(mut state) = state.lock() {
                    state.snapshot = snapshot;
                    state.action_catalog = action_catalog;
                    state.graph_rows = graph_rows;
                    state.busy = false;
                    state.current_operation = None;
                    match result {
                        Ok(message) => {
                            state.last_message = message.or(Some(label));
                            state.last_error = None;
                        }
                        Err(error) => {
                            state.last_message = None;
                            state.last_error = Some(error.into());
                        }
                    }
                }
            }
        }
    }
}

fn quote_arg(raw: &str) -> String {
    if raw.is_empty()
        || raw
            .chars()
            .any(|ch| ch.is_whitespace() || matches!(ch, '"' | '\'' | '\\'))
    {
        let escaped = raw.replace('\\', "\\\\").replace('"', "\\\"");
        format!("\"{escaped}\"")
    } else {
        raw.to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn unique_temp_dir(prefix: &str) -> std::path::PathBuf {
        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_nanos())
            .unwrap_or(0);
        std::env::temp_dir().join(format!("branchforge-desktop-{prefix}-{nanos}"))
    }

    fn test_config(root: &Path) -> ConsoleRunnerConfig {
        ConsoleRunnerConfig {
            cwd: root.to_path_buf(),
            plugins_root: root.join("plugins"),
            auto_render: false,
        }
    }

    #[test]
    fn exposes_structured_action_catalog() {
        let root = unique_temp_dir("catalog");
        assert!(std::fs::create_dir_all(&root).is_ok());
        let adapter = DesktopRuntimeAdapter::new(test_config(&root));
        let state = adapter.state();

        assert!(
            state
                .action_catalog
                .iter()
                .any(|item| item.action_id == "repo.open" && item.owner == "repo_manager")
        );
        assert!(
            state
                .action_catalog
                .iter()
                .any(|item| item.action_id == "commit.create" && !item.enabled)
        );

        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn opens_repo_through_host_runtime() {
        let root = unique_temp_dir("open-root");
        let repo_dir = root.join("repo with spaces");
        assert!(std::fs::create_dir_all(&repo_dir).is_ok());
        assert!(git_service::run_git(&repo_dir, &["init"]).is_ok());
        assert!(std::fs::write(repo_dir.join("README.md"), "hello\n").is_ok());

        let adapter = DesktopRuntimeAdapter::new(test_config(&root));
        let opened = adapter.open_repo(&repo_dir);
        assert!(opened.is_ok());
        assert!(adapter.wait_for_idle(Duration::from_secs(5)));
        let state = adapter.state();

        assert!(state.last_error.is_none());
        assert!(state.snapshot.repo.is_some());
        assert!(
            state
                .snapshot
                .status
                .untracked
                .iter()
                .any(|path| path == "README.md")
        );
        assert!(
            state
                .action_catalog
                .iter()
                .any(|item| item.action_id == "commit.amend" && item.enabled)
        );

        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn previews_and_loads_reflog_through_host_runtime() {
        let root = unique_temp_dir("safety-runtime");
        let repo_dir = root.join("repo");
        assert!(std::fs::create_dir_all(&repo_dir).is_ok());
        assert!(git_service::run_git(&repo_dir, &["init"]).is_ok());
        assert!(
            git_service::run_git(&repo_dir, &["config", "user.email", "dev@example.com"]).is_ok()
        );
        assert!(git_service::run_git(&repo_dir, &["config", "user.name", "Dev User"]).is_ok());
        assert!(std::fs::write(repo_dir.join("README.md"), "hello\n").is_ok());
        assert!(git_service::stage_paths(&repo_dir, &["README.md".to_string()]).is_ok());
        assert!(git_service::commit_create(&repo_dir, "base").is_ok());

        let adapter = DesktopRuntimeAdapter::new(test_config(&root));
        assert!(adapter.open_repo(&repo_dir).is_ok());
        assert!(adapter.wait_for_idle(Duration::from_secs(5)));

        let preview = adapter
            .preview_action("reset.hard", &["HEAD".to_string()])
            .expect("preview");
        assert_eq!(preview.operation, "reset.hard");
        assert!(!preview.git_commands.is_empty());

        let reflog = adapter.load_reflog("HEAD", 10).expect("reflog");
        assert!(!reflog.is_empty());
        assert_eq!(adapter.state().reflog_entries, reflog);

        let _ = std::fs::remove_dir_all(&root);
    }
}
