use std::collections::HashMap;
use std::io::{BufRead, Write};
use std::path::{Path, PathBuf};

use action_engine::{ActionRequest, validate_action};
use graph_model::{
    GraphCommit, GraphInputCommit, GraphRef, GraphRefKind, GraphRefLabel, build_graph,
};
use job_system::{JobExecutionResult, JobLock, JobRequest, execute_job_op};
use plugin_api::{ActionEffects, ActionSpec, ConfirmPolicy, DangerLevel};
use plugin_host::{
    PluginManagerError, bootstrap_plugin_runtime, branches_registration_payload,
    compare_registration_payload, diagnostics_registration_payload, discover_local_plugins,
    history_registration_payload, install_local_plugin, install_registry_plugin,
    invoke_plugin_action, list_installed_plugins, remove_local_plugin,
    repo_manager_registration_payload, set_plugin_enabled, spawn_installed_plugin_process,
    status_registration_payload, tags_registration_payload,
};
use state_store::{
    AuthStatus, BranchStack, BranchStackEntry, BranchStackState, CheckStatus, CommitImpact,
    DiffSource, DiffState, ExplainTemplate, FileImpact, ImpactLevel, ImpactSummary,
    InstalledPluginRecord, OperationPreview, PluginSecurityRecord, PluginSignatureStatus,
    PluginTrustLevel, PreviewWarning, PreviewWarningLevel, ProviderRepository, PullRequestState,
    PullRequestStateSnapshot, PullRequestSummary, RefImpact, RemoteImpact, RepoBranchSummary,
    RepoStatusSummary, ReviewState, StackEntryStatus, StateStore, Workspace, WorkspaceJobResult,
    WorkspaceRepo, WorkspaceState,
};

use crate::credentials::{CredentialVault, StoredCredential, provider_from_host};
use crate::errors::{ErrorCategory, UserFacingError, translate_job_error};
use crate::operations;
use crate::provider_api::{ProviderApiConfig, list_pull_requests};
use crate::recent_repos::persist_recent_repo;
use crate::run_rebase_beta_smoke;

#[cfg(test)]
use std::io::Cursor;

#[derive(Debug, Clone)]
pub struct ConsoleRunnerConfig {
    pub cwd: PathBuf,
    pub plugins_root: PathBuf,
    pub auth_metadata_path: Option<PathBuf>,
    pub auth_file_store: Option<PathBuf>,
    pub github_api_base: Option<String>,
    pub gitlab_api_base: Option<String>,
    pub auto_render: bool,
}

impl ConsoleRunnerConfig {
    pub fn from_current_env() -> Result<Self, String> {
        let cwd = std::env::current_dir().map_err(|err| err.to_string())?;
        let plugins_root = std::env::var_os("BRANCHFORGE_PLUGINS_ROOT")
            .map(PathBuf::from)
            .unwrap_or_else(|| cwd.join("target/tmp/console-runner/plugins"));
        Ok(Self {
            cwd,
            plugins_root,
            auth_metadata_path: std::env::var_os("BRANCHFORGE_AUTH_METADATA").map(PathBuf::from),
            auth_file_store: std::env::var_os("BRANCHFORGE_AUTH_FILE_STORE").map(PathBuf::from),
            github_api_base: std::env::var("BRANCHFORGE_GITHUB_API_BASE").ok(),
            gitlab_api_base: std::env::var("BRANCHFORGE_GITLAB_API_BASE").ok(),
            auto_render: true,
        })
    }
}

impl Default for ConsoleRunnerConfig {
    fn default() -> Self {
        Self::from_current_env().unwrap_or_else(|_| Self {
            cwd: PathBuf::from("."),
            plugins_root: PathBuf::from("target/tmp/console-runner/plugins"),
            auth_metadata_path: None,
            auth_file_store: None,
            github_api_base: None,
            gitlab_api_base: None,
            auto_render: true,
        })
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ConsoleSessionOutput {
    pub stdout: String,
    pub stderr: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HostRuntimeError {
    pub title: String,
    pub message: String,
    pub detail: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum ConsoleCommand {
    Empty,
    Help,
    Open {
        path: String,
    },
    Panel {
        panel: PanelKind,
    },
    Show,
    Actions,
    Ops,
    Run {
        target: String,
        args: Vec<String>,
        confirmed: bool,
    },
    Select {
        target: SelectionTarget,
        value: String,
    },
    Refresh,
    Plugin {
        op: PluginOp,
        confirmed: bool,
    },
    Quit,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum SelectionTarget {
    File,
    Commit,
    Branch,
    Plugin,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum PanelKind {
    Status,
    History,
    Branches,
    Tags,
    Compare,
    Diagnostics,
    Logs,
}

impl PanelKind {
    fn parse(raw: &str) -> Option<Self> {
        match raw {
            "status" => Some(Self::Status),
            "history" => Some(Self::History),
            "branches" => Some(Self::Branches),
            "tags" => Some(Self::Tags),
            "compare" => Some(Self::Compare),
            "diagnostics" => Some(Self::Diagnostics),
            "logs" => Some(Self::Logs),
            _ => None,
        }
    }

    fn as_str(self) -> &'static str {
        match self {
            Self::Status => "status",
            Self::History => "history",
            Self::Branches => "branches",
            Self::Tags => "tags",
            Self::Compare => "compare",
            Self::Diagnostics => "diagnostics",
            Self::Logs => "logs",
        }
    }

    fn view_id(self) -> &'static str {
        match self {
            Self::Status => "status.panel",
            Self::History => "history.panel",
            Self::Branches => "branches.panel",
            Self::Tags => "tags.panel",
            Self::Compare => "compare.panel",
            Self::Diagnostics => "diagnostics.panel",
            Self::Logs => "logs.panel",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum PluginOp {
    List,
    Discover {
        registry_path: Option<String>,
    },
    Marketplace {
        registry_path: Option<String>,
    },
    Install {
        package_dir: String,
    },
    InstallRegistry {
        plugin_id: String,
        registry_path: Option<String>,
    },
    Update {
        plugin_id: String,
        registry_path: Option<String>,
    },
    Enable {
        plugin_id: Option<String>,
    },
    Disable {
        plugin_id: Option<String>,
    },
    Remove {
        plugin_id: Option<String>,
    },
}

#[derive(Debug, Clone)]
struct CatalogAction {
    owner: String,
    spec: ActionSpec,
}

#[derive(Debug)]
struct DynamicPluginRuntime {
    process: plugin_host::PluginProcess,
    session: plugin_host::RuntimeSession,
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum ReplayableRun {
    Run { target: String, args: Vec<String> },
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct CommandResult {
    message: Option<String>,
    render: bool,
    exit: bool,
}

struct ConsoleRunner {
    config: ConsoleRunnerConfig,
    store: StateStore,
    repo_dir: Option<PathBuf>,
    dynamic_plugins: HashMap<String, DynamicPluginRuntime>,
    actions: Vec<CatalogAction>,
    last_message: Option<String>,
    last_replayable: Option<ReplayableRun>,
}

pub fn run_console_app() -> Result<(), String> {
    let config = ConsoleRunnerConfig::from_current_env()?;
    let stdin = std::io::stdin();
    let stdout = std::io::stdout();
    let stderr = std::io::stderr();

    run_console_session(stdin.lock(), stdout.lock(), stderr.lock(), config)
}

pub fn run_console_session<R: BufRead, W: Write, E: Write>(
    mut input: R,
    mut output: W,
    mut debug_output: E,
    config: ConsoleRunnerConfig,
) -> Result<(), String> {
    let mut runner = ConsoleRunner::new(config);

    writeln!(output, "Branchforge Console Runner").map_err(|err| err.to_string())?;
    writeln!(
        output,
        "Type `help` for commands. Use `run --confirm ...` for destructive actions."
    )
    .map_err(|err| err.to_string())?;
    if runner.config.auto_render {
        writeln!(output, "{}", runner.render_screen()).map_err(|err| err.to_string())?;
    }

    loop {
        write!(output, "bf> ").map_err(|err| err.to_string())?;
        output.flush().map_err(|err| err.to_string())?;

        let mut line = String::new();
        let read = input.read_line(&mut line).map_err(|err| err.to_string())?;
        if read == 0 {
            break;
        }

        let command = match parse_command_line(&line) {
            Ok(command) => command,
            Err(message) => {
                let error = invalid_input_error(&message);
                write_user_error(&mut output, &mut debug_output, &error)
                    .map_err(|err| err.to_string())?;
                continue;
            }
        };

        match runner.execute(command) {
            Ok(result) => {
                if let Some(message) = result.message.as_deref() {
                    writeln!(output, "{message}").map_err(|err| err.to_string())?;
                }
                if result.render {
                    writeln!(output, "{}", runner.render_screen())
                        .map_err(|err| err.to_string())?;
                }
                if result.exit {
                    break;
                }
            }
            Err(error) => {
                write_user_error(&mut output, &mut debug_output, &error)
                    .map_err(|err| err.to_string())?;
            }
        }
    }

    Ok(())
}

pub fn run_console_command(
    command_line: &str,
    config: ConsoleRunnerConfig,
    render_result: bool,
) -> Result<ConsoleSessionOutput, String> {
    let mut stdout = Vec::new();
    let mut stderr = Vec::new();
    let mut runner = ConsoleRunner::new(config);

    let command = match parse_command_line(command_line) {
        Ok(command) => command,
        Err(message) => {
            let error = invalid_input_error(&message);
            write_user_error(&mut stdout, &mut stderr, &error).map_err(|err| err.to_string())?;
            return Ok(ConsoleSessionOutput {
                stdout: String::from_utf8(stdout).map_err(|err| err.to_string())?,
                stderr: String::from_utf8(stderr).map_err(|err| err.to_string())?,
            });
        }
    };

    match runner.execute(command) {
        Ok(result) => {
            if let Some(message) = result.message.as_deref() {
                writeln!(stdout, "{message}").map_err(|err| err.to_string())?;
            }
            if render_result && result.render {
                writeln!(stdout, "{}", runner.render_screen()).map_err(|err| err.to_string())?;
            }
        }
        Err(error) => {
            write_user_error(&mut stdout, &mut stderr, &error).map_err(|err| err.to_string())?;
        }
    }

    Ok(ConsoleSessionOutput {
        stdout: String::from_utf8(stdout).map_err(|err| err.to_string())?,
        stderr: String::from_utf8(stderr).map_err(|err| err.to_string())?,
    })
}

pub struct HostRuntime {
    runner: ConsoleRunner,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HostActionCatalogItem {
    pub owner: String,
    pub action_id: String,
    pub title: String,
    pub danger: DangerLevel,
    pub confirm_policy: ConfirmPolicy,
    pub enabled: bool,
    pub disabled_reason: Option<String>,
    pub has_params: bool,
    pub explain: Option<ExplainTemplate>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HostReflogEntry {
    pub oid: String,
    pub selector: String,
    pub time: String,
    pub message: String,
}

impl HostRuntime {
    pub fn new(config: ConsoleRunnerConfig) -> Self {
        Self {
            runner: ConsoleRunner::new(config),
        }
    }

    pub fn submit_line(&mut self, command_line: &str) -> Result<Option<String>, HostRuntimeError> {
        let command = parse_command_line(command_line).map_err(|message| HostRuntimeError {
            title: "Invalid input".to_string(),
            message,
            detail: None,
        })?;
        let result = self
            .runner
            .execute(command)
            .map_err(|error| HostRuntimeError {
                title: error.title,
                message: error.message,
                detail: error.detail,
            })?;
        Ok(result.message)
    }

    pub fn snapshot(&self) -> state_store::StoreSnapshot {
        self.runner.store.snapshot().clone()
    }

    pub fn render_screen(&self) -> String {
        self.runner.render_screen()
    }

    pub fn render_actions(&self) -> String {
        self.runner.render_actions()
    }

    pub fn action_catalog(&self) -> Vec<HostActionCatalogItem> {
        self.runner.action_catalog_items()
    }

    pub fn preview_action(
        &self,
        action_id: &str,
        args: &[String],
    ) -> Result<OperationPreview, HostRuntimeError> {
        self.runner
            .preview_operation(action_id, args)
            .map_err(|error| HostRuntimeError {
                title: error.title,
                message: error.message,
                detail: error.detail,
            })
    }

    pub fn explain_action(&self, action_id: &str) -> Option<ExplainTemplate> {
        explain_template_for_action(action_id)
    }

    pub fn reflog_entries(
        &self,
        reference: &str,
        limit: usize,
    ) -> Result<Vec<HostReflogEntry>, HostRuntimeError> {
        self.runner
            .reflog_entries(reference, limit)
            .map_err(|error| HostRuntimeError {
                title: error.title,
                message: error.message,
                detail: error.detail,
            })
    }

    pub fn history_graph(&self) -> Result<Vec<GraphCommit>, HostRuntimeError> {
        self.runner
            .history_graph()
            .map_err(|error| HostRuntimeError {
                title: error.title,
                message: error.message,
                detail: error.detail,
            })
    }

    pub fn ops_catalog(&self) -> String {
        ops_text()
    }
}

#[cfg(test)]
pub fn run_scripted_console_session(
    script: &str,
    config: ConsoleRunnerConfig,
) -> Result<ConsoleSessionOutput, String> {
    let mut stdout = Vec::new();
    let mut stderr = Vec::new();
    run_console_session(
        Cursor::new(script.as_bytes()),
        &mut stdout,
        &mut stderr,
        config,
    )?;
    Ok(ConsoleSessionOutput {
        stdout: String::from_utf8(stdout).map_err(|err| err.to_string())?,
        stderr: String::from_utf8(stderr).map_err(|err| err.to_string())?,
    })
}

impl ConsoleRunner {
    fn new(config: ConsoleRunnerConfig) -> Self {
        let mut store = StateStore::new();
        let _ = store.restore_workspaces(&workspace_store_path_for(&config.cwd));
        Self {
            config,
            store,
            repo_dir: None,
            dynamic_plugins: HashMap::new(),
            actions: build_builtin_catalog_actions(),
            last_message: Some("runner initialized".to_string()),
            last_replayable: None,
        }
    }

    fn execute(&mut self, command: ConsoleCommand) -> Result<CommandResult, UserFacingError> {
        match command {
            ConsoleCommand::Empty => Ok(CommandResult {
                message: None,
                render: false,
                exit: false,
            }),
            ConsoleCommand::Help => Ok(CommandResult {
                message: Some(help_text()),
                render: false,
                exit: false,
            }),
            ConsoleCommand::Open { path } => {
                let message = self.open_repo(&path)?;
                Ok(self.finish_success(message, true))
            }
            ConsoleCommand::Panel { panel } => {
                let message = self.switch_panel(panel)?;
                Ok(self.finish_success(message, true))
            }
            ConsoleCommand::Show => Ok(CommandResult {
                message: None,
                render: true,
                exit: false,
            }),
            ConsoleCommand::Actions => Ok(CommandResult {
                message: Some(self.render_actions()),
                render: false,
                exit: false,
            }),
            ConsoleCommand::Ops => Ok(CommandResult {
                message: Some(ops_text()),
                render: false,
                exit: false,
            }),
            ConsoleCommand::Run {
                target,
                args,
                confirmed,
            } => {
                let message = self.run_target(&target, &args, confirmed)?;
                Ok(self.finish_success(message, true))
            }
            ConsoleCommand::Select { target, value } => {
                let message = self.select_target(target, &value)?;
                Ok(self.finish_success(message, true))
            }
            ConsoleCommand::Refresh => {
                let message = self.refresh()?;
                Ok(self.finish_success(message, true))
            }
            ConsoleCommand::Plugin { op, confirmed } => {
                let message = self.run_plugin_op(op, confirmed)?;
                Ok(self.finish_success(message, true))
            }
            ConsoleCommand::Quit => Ok(CommandResult {
                message: Some("bye".to_string()),
                render: false,
                exit: true,
            }),
        }
    }

    fn finish_success(&mut self, message: String, render: bool) -> CommandResult {
        self.last_message = Some(message.clone());
        CommandResult {
            message: Some(message),
            render,
            exit: false,
        }
    }

    fn render_screen(&self) -> String {
        let window = ui_shell::render_window(&self.store, &self.contextual_palette_items());
        let selected_files = if self.store.snapshot().selection.selected_paths.is_empty() {
            "<none>".to_string()
        } else {
            self.store.snapshot().selection.selected_paths.join(", ")
        };
        let selected_commit = self
            .store
            .snapshot()
            .selection
            .selected_commit_oid
            .clone()
            .unwrap_or_else(|| "<none>".to_string());
        let selected_branch = self
            .store
            .snapshot()
            .selection
            .selected_branch
            .clone()
            .unwrap_or_else(|| "<none>".to_string());
        let selected_plugin = self
            .store
            .snapshot()
            .selection
            .selected_plugin_id
            .clone()
            .unwrap_or_else(|| "<none>".to_string());
        let repo = self
            .repo_dir
            .as_ref()
            .map(|path| path.display().to_string())
            .unwrap_or_else(|| "<not opened>".to_string());
        let active_panel = self
            .store
            .snapshot()
            .active_view
            .clone()
            .unwrap_or_else(|| "<none>".to_string());
        let last = self
            .last_message
            .clone()
            .unwrap_or_else(|| "<none>".to_string());

        format!(
            "[runner]\nrepo: {repo}\nactive_panel: {active_panel}\nplugins_root: {}\nselection.files: {selected_files}\nselection.commit: {selected_commit}\nselection.branch: {selected_branch}\nselection.plugin: {selected_plugin}\nlast: {last}\n{window}",
            self.config.plugins_root.display()
        )
    }

    fn contextual_palette_items(&self) -> Vec<ui_shell::palette::PaletteItem> {
        let active_owner = self
            .store
            .snapshot()
            .active_view
            .as_deref()
            .and_then(view_to_owner);
        let actions = self
            .actions
            .iter()
            .filter(|action| {
                if action.spec.action_id == "repo.open" {
                    return true;
                }
                match active_owner {
                    Some(owner) => action.owner == owner,
                    None => action.owner == "repo_manager",
                }
            })
            .map(|action| action.spec.clone())
            .collect::<Vec<_>>();
        ui_shell::palette::build_palette(&actions, "", self.repo_dir.is_some())
    }

    fn render_actions(&self) -> String {
        let mut lines = Vec::new();
        lines.push("Actions".to_string());
        for action in &self.actions {
            let (enabled, reason) = self.action_availability(&action.spec);
            let state = if enabled { "on " } else { "off" };
            let confirm = match action.spec.confirm_policy {
                plugin_api::ConfirmPolicy::Never => "confirm=never",
                plugin_api::ConfirmPolicy::Always => "confirm=always",
                plugin_api::ConfirmPolicy::OnDanger => "confirm=high-danger",
            };
            let mut line = format!(
                "{state} {:<26} owner={:<12} {} {}",
                action.spec.action_id, action.owner, confirm, action.spec.title
            );
            if let Some(reason) = reason {
                line.push_str(&format!(" | reason: {reason}"));
            }
            lines.push(line);
        }
        lines.join("\n")
    }

    fn action_catalog_items(&self) -> Vec<HostActionCatalogItem> {
        self.actions
            .iter()
            .map(|action| {
                let (enabled, disabled_reason) = self.action_availability(&action.spec);
                HostActionCatalogItem {
                    owner: action.owner.clone(),
                    action_id: action.spec.action_id.clone(),
                    title: action.spec.title.clone(),
                    danger: action.spec.effective_danger(),
                    confirm_policy: action.spec.confirm_policy.clone(),
                    enabled,
                    disabled_reason,
                    has_params: action.spec.params_schema.is_some(),
                    explain: explain_template_for_action(&action.spec.action_id),
                }
            })
            .collect()
    }

    fn history_graph(&self) -> Result<Vec<GraphCommit>, UserFacingError> {
        let Some(repo_dir) = self.repo_dir.as_ref() else {
            return Ok(Vec::new());
        };
        let visible_count = self.store.snapshot().history.commits.len().max(100);
        let graph_log =
            git_service::commit_graph_page(repo_dir, 0, visible_count).map_err(|err| {
                UserFacingError::with_category(
                    "History graph failed",
                    "Could not load commit graph metadata.",
                    Some(format!("{err:?}")),
                    ErrorCategory::Git,
                )
            })?;

        let mut refs = Vec::new();
        let inputs = graph_log
            .into_iter()
            .map(|commit| {
                refs.extend(commit.refs.iter().filter_map(|label| {
                    map_graph_ref_label(label).map(|mapped| GraphRef {
                        oid: commit.oid.clone(),
                        label: mapped,
                    })
                }));
                GraphInputCommit {
                    short_oid: commit.oid.chars().take(8).collect(),
                    oid: commit.oid,
                    summary: commit.summary,
                    author: commit.author,
                    time: commit.time,
                    parents: commit.parents,
                }
            })
            .collect::<Vec<_>>();

        Ok(build_graph(&inputs, &refs))
    }

    fn reflog_entries(
        &self,
        reference: &str,
        limit: usize,
    ) -> Result<Vec<HostReflogEntry>, UserFacingError> {
        let repo_dir = self.require_repo_dir()?;
        let entries = git_service::reflog_entries(&repo_dir, reference, limit).map_err(|err| {
            UserFacingError::with_category(
                "Reflog failed",
                "Could not load reflog entries.",
                Some(format!("{err:?}")),
                ErrorCategory::Git,
            )
        })?;
        Ok(entries
            .into_iter()
            .map(|entry| HostReflogEntry {
                oid: entry.oid,
                selector: entry.selector,
                time: entry.time,
                message: entry.message,
            })
            .collect())
    }

    fn preview_operation(
        &self,
        action_id: &str,
        args: &[String],
    ) -> Result<OperationPreview, UserFacingError> {
        let Some(spec) = self.find_action(action_id) else {
            return Err(UserFacingError::with_category(
                "Preview unavailable",
                &format!("Unknown action/op `{action_id}`."),
                None,
                ErrorCategory::System,
            ));
        };
        let explain = explain_template_for_action(action_id);
        let mut preview =
            base_operation_preview(action_id, spec.effective_danger(), explain.as_ref(), args);

        match action_id {
            "reset.soft" | "reset.mixed" | "reset.hard" => {
                self.enrich_reset_preview(action_id, args, &mut preview)?;
            }
            "branch.delete" => {
                self.enrich_branch_delete_preview(args, &mut preview)?;
            }
            "tag.delete" => {
                self.enrich_tag_delete_preview(args, &mut preview)?;
            }
            "file.discard" | "file.discard_hunk" | "file.discard_lines" => {
                self.enrich_discard_preview(action_id, args, &mut preview)?;
            }
            "stash.pop" | "stash.drop" => {
                self.enrich_stash_preview(action_id, args, &mut preview);
            }
            "merge.execute" => {
                self.enrich_merge_preview(args, &mut preview)?;
            }
            "rebase.execute" | "rebase.interactive" => {
                self.enrich_rebase_preview(args, &mut preview);
            }
            "conflict.abort" | "rebase.abort" | "merge.abort" => {
                self.enrich_abort_preview(action_id, &mut preview);
            }
            "remote.push_force_with_lease" | "remote.remove" => {
                self.enrich_remote_preview(action_id, args, &mut preview);
            }
            _ => {}
        }

        Ok(preview)
    }

    fn enrich_remote_preview(
        &self,
        action_id: &str,
        args: &[String],
        preview: &mut OperationPreview,
    ) {
        match action_id {
            "remote.push_force_with_lease" => {
                let branch = args
                    .get(1)
                    .cloned()
                    .or_else(|| {
                        self.store
                            .snapshot()
                            .repo
                            .as_ref()
                            .and_then(|repo| repo.head.clone())
                    })
                    .unwrap_or_else(|| "current branch".to_string());
                let remote = args
                    .first()
                    .cloned()
                    .unwrap_or_else(|| "configured upstream".to_string());
                preview.summary =
                    format!("Force push {branch} to {remote} using --force-with-lease.");
                preview.remote_impact = Some(RemoteImpact {
                    remote,
                    summary: "Remote branch history can be rewritten if the lease still matches."
                        .to_string(),
                });
                preview.warnings.push(warning(
                    PreviewWarningLevel::Danger,
                    "Coordinate with collaborators before rewriting a shared branch.",
                ));
                preview.recommended_action = Some(
                    "Fetch first, inspect ahead/behind counts, and keep the journal backup ref."
                        .to_string(),
                );
            }
            "remote.remove" => {
                let remote = args
                    .first()
                    .cloned()
                    .unwrap_or_else(|| "selected remote".to_string());
                preview.summary = format!("Remove remote `{remote}` from local repository config.");
                preview.remote_impact = Some(RemoteImpact {
                    remote,
                    summary: "Local remote configuration is removed; remote branches may disappear after pruning."
                        .to_string(),
                });
                preview.warnings.push(warning(
                    PreviewWarningLevel::Warning,
                    "Fetch/push shortcuts that reference this remote will stop working.",
                ));
            }
            _ => {}
        }
    }

    fn enrich_reset_preview(
        &self,
        action_id: &str,
        args: &[String],
        preview: &mut OperationPreview,
    ) -> Result<(), UserFacingError> {
        let repo_dir = self.require_repo_dir()?;
        let target = args.first().map(String::as_str).unwrap_or("HEAD");
        let target_oid = git_service::resolve_ref_oid(&repo_dir, target).ok();
        let current_oid = git_service::resolve_ref_oid(&repo_dir, "HEAD").ok();
        let head_name = self
            .store
            .snapshot()
            .repo
            .as_ref()
            .and_then(|repo| repo.head.clone())
            .unwrap_or_else(|| "HEAD".to_string());
        preview.summary = match action_id {
            "reset.soft" => {
                format!("Move {head_name} to {target}; keep index and working tree as-is.")
            }
            "reset.mixed" => {
                format!("Move {head_name} to {target} and reset the index.")
            }
            "reset.hard" => {
                format!("Move {head_name} to {target} and overwrite index and working tree.")
            }
            _ => preview.summary.clone(),
        };
        preview.affected_refs.push(RefImpact {
            name: head_name,
            before: current_oid,
            after: target_oid,
            impact: "branch tip moves".to_string(),
        });
        preview.index_impact = match action_id {
            "reset.soft" => impact(ImpactLevel::None, "Index is preserved."),
            "reset.mixed" => impact(ImpactLevel::Destructive, "Index changes are replaced."),
            "reset.hard" => impact(ImpactLevel::Destructive, "Index changes are replaced."),
            _ => preview.index_impact.clone(),
        };
        preview.worktree_impact = match action_id {
            "reset.hard" => impact(
                ImpactLevel::Destructive,
                "Working tree changes may be lost.",
            ),
            _ => impact(ImpactLevel::None, "Working tree files are preserved."),
        };
        if action_id == "reset.hard" {
            preview.warnings.push(warning(
                PreviewWarningLevel::Danger,
                "Uncommitted worktree and staged changes can be overwritten.",
            ));
            preview.recommended_action = Some(
                "Create a backup ref or stash uncommitted work before continuing.".to_string(),
            );
        }
        Ok(())
    }

    fn enrich_branch_delete_preview(
        &self,
        args: &[String],
        preview: &mut OperationPreview,
    ) -> Result<(), UserFacingError> {
        let repo_dir = self.require_repo_dir()?;
        let branch = args
            .first()
            .cloned()
            .or_else(|| self.store.snapshot().selection.selected_branch.clone())
            .ok_or_else(|| invalid_input_error("branch.delete requires a branch name"))?;
        let oid = git_service::resolve_ref_oid(&repo_dir, &branch).ok();
        let merged = git_service::branch_is_merged(&repo_dir, &branch).unwrap_or(false);
        preview.summary = if merged {
            format!("Delete local branch {branch}. It appears merged into the current HEAD.")
        } else {
            format!(
                "Delete local branch {branch}. It may contain commits not merged into the current HEAD."
            )
        };
        preview.affected_refs.push(RefImpact {
            name: format!("refs/heads/{branch}"),
            before: oid,
            after: None,
            impact: "branch ref deleted".to_string(),
        });
        if !merged {
            preview.warnings.push(warning(
                PreviewWarningLevel::Danger,
                "Branch is not merged into the current HEAD; commits may become hard to find without a backup ref.",
            ));
        }
        preview.recommended_action = Some(
            "A BranchForge backup ref will be created before delete when recovery is enabled."
                .to_string(),
        );
        Ok(())
    }

    fn enrich_tag_delete_preview(
        &self,
        args: &[String],
        preview: &mut OperationPreview,
    ) -> Result<(), UserFacingError> {
        let repo_dir = self.require_repo_dir()?;
        let tag = args
            .first()
            .ok_or_else(|| invalid_input_error("tag.delete requires a tag name"))?;
        preview.affected_refs.push(RefImpact {
            name: format!("refs/tags/{tag}"),
            before: git_service::resolve_ref_oid(&repo_dir, tag).ok(),
            after: None,
            impact: "tag ref deleted".to_string(),
        });
        preview.summary = format!("Delete local tag {tag}.");
        preview.recommended_action = Some(
            "A BranchForge backup ref will be created before delete when recovery is enabled."
                .to_string(),
        );
        Ok(())
    }

    fn enrich_discard_preview(
        &self,
        action_id: &str,
        args: &[String],
        preview: &mut OperationPreview,
    ) -> Result<(), UserFacingError> {
        let files = if action_id == "file.discard" {
            if args.is_empty() {
                self.selected_files()?
            } else {
                args.to_vec()
            }
        } else {
            vec![
                args.first()
                    .cloned()
                    .ok_or_else(|| invalid_input_error("discard preview requires file path"))?,
            ]
        };
        preview.affected_files = files
            .into_iter()
            .map(|path| FileImpact {
                path,
                impact: "worktree content discarded".to_string(),
                detail: match action_id {
                    "file.discard_hunk" => args.get(1).map(|idx| format!("hunk {idx}")),
                    "file.discard_lines" => {
                        Some(format!("{} selected line(s)", args.len().saturating_sub(2)))
                    }
                    _ => None,
                },
            })
            .collect();
        preview.worktree_impact = impact(
            ImpactLevel::Destructive,
            "Selected worktree changes are discarded.",
        );
        preview.warnings.push(warning(
            PreviewWarningLevel::Danger,
            "Discarding worktree changes cannot always be restored unless a patch snapshot exists.",
        ));
        Ok(())
    }

    fn enrich_stash_preview(
        &self,
        action_id: &str,
        args: &[String],
        preview: &mut OperationPreview,
    ) {
        let reference = args.first().map(String::as_str).unwrap_or("stash@{0}");
        preview.summary = match action_id {
            "stash.pop" => {
                format!(
                    "Apply {reference} to the working tree and remove it from the stash list if successful."
                )
            }
            "stash.drop" => format!("Remove {reference} from the stash list."),
            _ => preview.summary.clone(),
        };
        preview.affected_refs.push(RefImpact {
            name: reference.to_string(),
            before: None,
            after: None,
            impact: if action_id == "stash.pop" {
                "stash applied and dropped".to_string()
            } else {
                "stash dropped".to_string()
            },
        });
        preview.worktree_impact = if action_id == "stash.pop" {
            impact(
                ImpactLevel::Write,
                "Stash contents will be applied to the working tree.",
            )
        } else {
            impact(
                ImpactLevel::None,
                "Working tree is not changed by stash drop.",
            )
        };
        preview.warnings.push(warning(
            PreviewWarningLevel::Warning,
            "Stash application can produce conflicts when changes overlap.",
        ));
    }

    fn enrich_merge_preview(
        &self,
        args: &[String],
        preview: &mut OperationPreview,
    ) -> Result<(), UserFacingError> {
        let source = args
            .first()
            .ok_or_else(|| invalid_input_error("merge.execute requires source ref"))?;
        let target = self
            .current_head_ref()
            .unwrap_or_else(|_| "HEAD".to_string());
        preview.summary = format!("Merge {source} into {target}.");
        preview.affected_refs.push(RefImpact {
            name: target,
            before: self
                .repo_dir
                .as_ref()
                .and_then(|repo| git_service::resolve_ref_oid(repo, "HEAD").ok()),
            after: None,
            impact: "target may advance or receive a merge commit".to_string(),
        });
        preview.worktree_impact = impact(
            ImpactLevel::Write,
            "Merge can update files and may leave conflict markers.",
        );
        preview.warnings.push(warning(
            PreviewWarningLevel::Warning,
            "Merge may stop for conflict resolution if branches touch the same lines.",
        ));
        Ok(())
    }

    fn enrich_rebase_preview(&self, args: &[String], preview: &mut OperationPreview) {
        let plan = self.store.snapshot().rebase.plan.clone();
        let base = plan
            .as_ref()
            .map(|plan| plan.base_ref.clone())
            .or_else(|| args.first().cloned())
            .unwrap_or_else(|| "<selected base>".to_string());
        preview.summary = format!("Rewrite selected commits on top of {base}.");
        if let Some(plan) = plan {
            preview.commits_rewritten = plan
                .entries
                .into_iter()
                .map(|entry| CommitImpact {
                    oid: entry.oid,
                    summary: entry.summary,
                    action: format!("{:?}", entry.action).to_lowercase(),
                })
                .collect();
            if let Some(warning_text) = plan.published_history_warning {
                preview
                    .warnings
                    .push(warning(PreviewWarningLevel::Danger, warning_text));
            }
        }
        preview.affected_refs.push(RefImpact {
            name: self
                .current_head_ref()
                .unwrap_or_else(|_| "HEAD".to_string()),
            before: self
                .repo_dir
                .as_ref()
                .and_then(|repo| git_service::resolve_ref_oid(repo, "HEAD").ok()),
            after: None,
            impact: "branch tip will be rewritten".to_string(),
        });
        preview.warnings.push(warning(
            PreviewWarningLevel::Danger,
            "Commit hashes will change. Published history may require force push coordination.",
        ));
        preview.recommended_action =
            Some("BranchForge will create a backup ref before executing the rebase.".to_string());
    }

    fn enrich_abort_preview(&self, action_id: &str, preview: &mut OperationPreview) {
        preview.summary = format!(
            "Abort the active {} session and return the repository to its pre-operation state if Git can do so.",
            action_id.trim_end_matches(".abort")
        );
        preview.worktree_impact = impact(
            ImpactLevel::Write,
            "Git may rewrite index and worktree files while aborting the session.",
        );
        preview.warnings.push(warning(
            PreviewWarningLevel::Warning,
            "Local conflict-resolution edits made during the session can be overwritten.",
        ));
    }

    fn action_availability(&self, spec: &ActionSpec) -> (bool, Option<String>) {
        let repo_open = self.repo_dir.is_some();
        if matches!(spec.when.as_deref(), Some("repo.is_open")) && !repo_open {
            return (false, Some("repository is not open".to_string()));
        }

        if let Some(owner) = self.action_owner_for(&spec.action_id)
            && let Some(status) = self.store.snapshot().plugins.iter().find(|status| {
                status.plugin_id == owner
                    && matches!(status.health, state_store::PluginHealth::Unavailable { .. })
            })
            && let state_store::PluginHealth::Unavailable { message } = &status.health
        {
            return (
                false,
                Some(format!("plugin {owner} unavailable: {message}")),
            );
        }

        let snapshot = self.store.snapshot();
        match spec.action_id.as_str() {
            "index.stage_selected" | "index.unstage_selected" | "file.discard"
                if snapshot.selection.selected_paths.is_empty() =>
            {
                return (false, Some("no selected files".to_string()));
            }
            "index.stage_hunk" | "index.stage_lines" | "file.discard_hunk"
            | "file.discard_lines"
                if snapshot.diff.hunks.is_empty()
                    || !matches!(
                        &snapshot.diff.source,
                        Some(state_store::DiffSource::Worktree { .. })
                    ) =>
            {
                return (
                    false,
                    Some("load a worktree diff with hunks first".to_string()),
                );
            }
            "index.unstage_hunk" | "index.unstage_lines"
                if snapshot.diff.hunks.is_empty()
                    || !matches!(
                        &snapshot.diff.source,
                        Some(state_store::DiffSource::Index { .. })
                    ) =>
            {
                return (
                    false,
                    Some("load an index diff with hunks first".to_string()),
                );
            }
            "commit.create" if snapshot.status.staged.is_empty() => {
                return (false, Some("no staged changes".to_string()));
            }
            "history.load_more" if snapshot.history.next_cursor.is_none() => {
                return (false, Some("no next history page".to_string()));
            }
            "history.select_commit" | "cherry_pick.commit" | "revert.commit"
                if snapshot.selection.selected_commit_oid.is_none() =>
            {
                return (false, Some("no selected commit".to_string()));
            }
            "history.file" | "blame.file" if snapshot.selection.selected_paths.is_empty() => {
                return (false, Some("no selected files".to_string()));
            }
            "branch.checkout" | "branch.rename" | "branch.delete"
                if snapshot.selection.selected_branch.is_none() =>
            {
                return (false, Some("no selected branch".to_string()));
            }
            "rebase.execute" if snapshot.rebase.plan.is_none() => {
                return (false, Some("no rebase plan".to_string()));
            }
            "rebase.plan.set_action" | "rebase.plan.move" | "rebase.plan.clear"
                if snapshot.rebase.plan.is_none() =>
            {
                return (false, Some("no rebase plan".to_string()));
            }
            "rebase.continue" | "rebase.skip" | "rebase.abort"
                if snapshot.rebase.session.is_none() =>
            {
                return (false, Some("no active rebase session".to_string()));
            }
            "conflict.focus" if snapshot.selection.selected_paths.is_empty() => {
                return (false, Some("no selected conflict files".to_string()));
            }
            "conflict.resolve.ours" | "conflict.resolve.theirs" | "conflict.mark_resolved"
                if snapshot.selection.selected_paths.is_empty() =>
            {
                return (false, Some("no selected conflict files".to_string()));
            }
            "conflict.continue" | "conflict.abort"
                if snapshot
                    .repo
                    .as_ref()
                    .and_then(|repo| repo.conflict_state.as_ref())
                    .is_none() =>
            {
                return (false, Some("no active conflict session".to_string()));
            }
            "plugin.enable" | "plugin.disable" | "plugin.remove"
                if snapshot.selection.selected_plugin_id.is_none() =>
            {
                return (
                    false,
                    Some("no selected plugin (or pass id explicitly)".to_string()),
                );
            }
            _ => {}
        }

        (true, None)
    }

    fn open_repo(&mut self, path: &str) -> Result<String, UserFacingError> {
        let repo_dir = resolve_path(&self.config.cwd, path);
        self.execute_job(&repo_dir, "repo.open", JobLock::Read, Vec::new(), false)?;
        self.repo_dir = self
            .store
            .repo()
            .map(|repo| PathBuf::from(repo.root.clone()))
            .or(Some(repo_dir.clone()));
        let _ = persist_recent_repo(&repo_dir);
        self.last_replayable = None;
        Ok(format!("opened repository {}", repo_dir.display()))
    }

    fn switch_panel(&mut self, panel: PanelKind) -> Result<String, UserFacingError> {
        self.store
            .set_active_view(Some(panel.view_id().to_string()));

        if self.repo_dir.is_some() {
            match panel {
                PanelKind::Status => {
                    self.execute_in_open_repo("status.refresh", Vec::new(), false)?;
                    self.execute_in_open_repo("refs.refresh", Vec::new(), false)?;
                    if !self.store.snapshot().selection.selected_paths.is_empty() {
                        let _ = self.refresh_selected_file_diff();
                    }
                }
                PanelKind::History => {
                    if self.store.snapshot().history.commits.is_empty()
                        && self
                            .store
                            .snapshot()
                            .repo
                            .as_ref()
                            .and_then(|repo| repo.head.as_ref())
                            .is_some()
                        && let Err(error) = self.execute_in_open_repo(
                            "history.page",
                            vec!["0".to_string(), "20".to_string()],
                            true,
                        )
                    {
                        let detail = error.detail.as_deref().unwrap_or_default();
                        let empty_history = detail.contains("does not have any commits yet")
                            || detail.contains("ambiguous argument 'HEAD'")
                            || detail.contains("bad revision 'HEAD'");
                        if empty_history {
                            self.store.clear_history();
                            self.last_replayable = None;
                        } else {
                            return Err(error);
                        }
                    }
                }
                PanelKind::Branches | PanelKind::Tags => {
                    self.execute_in_open_repo("refs.refresh", Vec::new(), false)?;
                }
                PanelKind::Compare => {
                    self.execute_in_open_repo("refs.refresh", Vec::new(), false)?;
                    if let (Some(base), Some(head)) = (
                        self.store.snapshot().compare.base_ref.clone(),
                        self.store.snapshot().compare.head_ref.clone(),
                    ) {
                        self.execute_in_open_repo("compare.refs", vec![base, head], true)?;
                    }
                }
                PanelKind::Diagnostics => {
                    self.sync_plugin_inventory()?;
                    self.update_journal_summary_diff();
                }
                PanelKind::Logs => {
                    self.sync_plugin_inventory()?;
                    self.update_journal_summary_diff();
                }
            }
        }

        Ok(format!("panel -> {}", panel.as_str()))
    }

    fn run_target(
        &mut self,
        target: &str,
        args: &[String],
        confirmed: bool,
    ) -> Result<String, UserFacingError> {
        let request = ActionRequest {
            action: target.to_string(),
            confirmed,
        };
        if !validate_action(&request) {
            return Err(invalid_input_error("action/op id cannot be empty"));
        }

        self.ensure_confirmation(target, args, confirmed)?;

        match target {
            "repo.open" => {
                let path = args.first().ok_or_else(|| {
                    invalid_input_error(
                        "repo.open requires repository path in `run repo.open <path>`",
                    )
                })?;
                self.open_repo(path)
            }
            "index.stage_selected" => {
                let selected = self.selected_files()?;
                self.execute_in_open_repo("index.stage_paths", selected, false)?;
                Ok("staged selected files".to_string())
            }
            "index.unstage_selected" => {
                let selected = self.selected_files()?;
                self.execute_in_open_repo("index.unstage_paths", selected, false)?;
                Ok("unstaged selected files".to_string())
            }
            "rebase.interactive" => self.run_interactive_rebase(args),
            "reset.soft" | "reset.mixed" | "reset.hard" => {
                let mode = target.trim_start_matches("reset.").to_string();
                let mut params = vec![mode];
                params.extend(args.iter().cloned());
                if params.len() == 1 {
                    params.push("HEAD".to_string());
                }
                self.execute_in_open_repo("reset.refs", params, false)?;
                Ok(format!("executed {target}"))
            }
            "diagnostics.journal_summary" => {
                self.sync_plugin_inventory()?;
                self.show_journal_summary();
                Ok("updated diagnostics journal summary".to_string())
            }
            "journal.open_entry" => {
                let entry_id = args.first().and_then(|value| value.parse::<u64>().ok());
                self.show_journal_entry(entry_id)?;
                Ok("opened journal entry details".to_string())
            }
            "journal.copy_details" => {
                let entry_id = args.first().and_then(|value| value.parse::<u64>().ok());
                let details = self.render_journal_entry_details(entry_id)?;
                self.store
                    .update_diff(render_text_diff("journal:entry_details", details.clone()));
                Ok(details)
            }
            "journal.export" => {
                let out_file = args
                    .first()
                    .map(|raw| resolve_path(&self.config.cwd, raw))
                    .unwrap_or_else(|| self.config.cwd.join("target/tmp/branchforge-journal.json"));
                if let Some(parent) = out_file.parent() {
                    std::fs::create_dir_all(parent).map_err(|err| {
                        UserFacingError::with_category(
                            "Journal export failed",
                            "Could not create journal export directory.",
                            Some(err.to_string()),
                            ErrorCategory::System,
                        )
                    })?;
                }
                self.store.persist_journal(&out_file).map_err(|err| {
                    UserFacingError::with_category(
                        "Journal export failed",
                        "Could not write journal export.",
                        Some(err),
                        ErrorCategory::System,
                    )
                })?;
                Ok(format!("exported journal to {}", out_file.display()))
            }
            "journal.clear_old_entries" => {
                let keep_latest = args
                    .first()
                    .and_then(|value| value.parse::<usize>().ok())
                    .unwrap_or(50);
                self.store.clear_old_journal_entries(keep_latest);
                Ok(format!("kept latest {keep_latest} journal entries"))
            }
            "journal.restore_ref" => {
                let params = self.resolve_restore_ref_args(args)?;
                self.execute_in_open_repo("recovery.restore_ref", params, false)?;
                Ok("restored ref from journal".to_string())
            }
            "journal.recover_operation" => {
                let params = self.resolve_recover_operation_args(args)?;
                self.execute_in_open_repo("recovery.create_branch_from_backup", params, false)?;
                Ok("created recovery branch from journal backup".to_string())
            }
            "conflict.focus" => {
                let repo_dir = self.require_repo_dir()?;
                let path = if let Some(raw) = args.first() {
                    normalize_repo_path(&repo_dir, raw)
                } else {
                    self.selected_file()?
                };
                self.store.update_selected_paths(vec![path.clone()]);
                self.execute_job(
                    &repo_dir,
                    "diff.worktree",
                    JobLock::Read,
                    vec![path.clone()],
                    true,
                )?;
                self.store
                    .set_active_view(Some(PanelKind::Branches.view_id().to_string()));
                Ok(format!("focused conflict file {path}"))
            }
            "workspace.create"
            | "workspace.add_repo"
            | "workspace.remove_repo"
            | "workspace.switch"
            | "workspace.switch_repo"
            | "workspace.refresh_all"
            | "workspace.fetch_all"
            | "workspace.persist"
            | "workspace.restore" => self.run_workspace_op(target, args),
            "auth.status" | "auth.login" | "auth.logout" | "auth.seed_git" => {
                self.run_auth_op(target, args)
            }
            "pr.detect_provider" | "pr.list" | "pr.create_url" | "pr.open" | "pr.checkout" => {
                self.run_pr_op(target, args)
            }
            "stack.create" | "stack.detect" | "stack.restack" => self.run_stack_op(target, args),
            "plugin.list" => self.run_plugin_op(PluginOp::List, confirmed),
            "plugin.discover" => {
                let registry_path = args.first().cloned();
                self.run_plugin_op(PluginOp::Discover { registry_path }, confirmed)
            }
            "plugin.marketplace" => {
                let registry_path = args.first().cloned();
                self.run_plugin_op(PluginOp::Marketplace { registry_path }, confirmed)
            }
            "plugin.install" => {
                let package_dir = args.first().cloned().ok_or_else(|| {
                    invalid_input_error("plugin.install requires package directory")
                })?;
                self.run_plugin_op(PluginOp::Install { package_dir }, confirmed)
            }
            "plugin.install_registry" => {
                let plugin_id = args.first().cloned().ok_or_else(|| {
                    invalid_input_error(
                        "plugin.install_registry requires plugin id: `run plugin.install_registry <plugin_id> [registry_path]`",
                    )
                })?;
                let registry_path = args.get(1).cloned();
                self.run_plugin_op(
                    PluginOp::InstallRegistry {
                        plugin_id,
                        registry_path,
                    },
                    confirmed,
                )
            }
            "plugin.update" => {
                let plugin_id = args.first().cloned().ok_or_else(|| {
                    invalid_input_error(
                        "plugin.update requires plugin id: `run plugin.update <plugin_id> [registry_path]`",
                    )
                })?;
                let registry_path = args.get(1).cloned();
                self.run_plugin_op(
                    PluginOp::Update {
                        plugin_id,
                        registry_path,
                    },
                    confirmed,
                )
            }
            "plugin.enable" => {
                let plugin_id = args.first().cloned();
                self.run_plugin_op(PluginOp::Enable { plugin_id }, confirmed)
            }
            "plugin.disable" => {
                let plugin_id = args.first().cloned();
                self.run_plugin_op(PluginOp::Disable { plugin_id }, confirmed)
            }
            "plugin.remove" => {
                let plugin_id = args.first().cloned();
                self.run_plugin_op(PluginOp::Remove { plugin_id }, confirmed)
            }
            "diagnostics.repo_capabilities" => {
                self.execute_in_open_repo(target, args.to_vec(), true)?;
                self.store
                    .set_active_view(Some(PanelKind::Diagnostics.view_id().to_string()));
                Ok("updated diagnostics repo capabilities".to_string())
            }
            "ops.check_deps"
            | "ops.dev_check"
            | "release.notes"
            | "release.sign"
            | "release.package_local"
            | "release.package"
            | "release.verify"
            | "verify.sprint22"
            | "verify.sprint23"
            | "verify.sprint24" => self.run_operational_op(target, args),
            _ if self.find_dynamic_plugin_owner(target).is_some()
                && !is_supported_direct_op(target) =>
            {
                self.invoke_dynamic_plugin_action(target, args)
            }
            _ if self.find_action(target).is_some() || is_supported_direct_op(target) => {
                let resolved_args = self.resolve_run_args(target, args)?;
                self.execute_in_open_repo(target, resolved_args, true)?;
                self.sync_active_panel_after_op(target);
                Ok(format!("executed {target}"))
            }
            _ => Err(UserFacingError::with_category(
                "Unsupported operation",
                &format!("Unknown action/op `{target}`."),
                None,
                ErrorCategory::System,
            )),
        }
    }

    fn run_interactive_rebase(&mut self, args: &[String]) -> Result<String, UserFacingError> {
        let repo_dir = self.require_repo_dir()?;
        let base_ref = args.first().cloned().ok_or_else(|| {
            invalid_input_error("rebase.interactive requires base ref: `run --confirm rebase.interactive <base-ref> [autosquash]`")
        })?;
        let autosquash = args.iter().any(|arg| arg == "autosquash");
        let preview = run_rebase_beta_smoke(&repo_dir).map_err(|detail| {
            UserFacingError::with_category(
                "Rebase preflight failed",
                "Interactive rebase is not ready.",
                Some(detail),
                ErrorCategory::Validation,
            )
        })?;
        if !preview.preflight.ok {
            return Err(UserFacingError::with_category(
                "Interactive rebase blocked",
                &preview.preview.summary,
                Some(preview.preview.warnings.join("; ")),
                ErrorCategory::Conflicts,
            ));
        }

        self.execute_job(
            &repo_dir,
            "rebase.plan.create",
            JobLock::RefsWrite,
            vec![base_ref.clone()],
            false,
        )?;
        let mut execute_args = Vec::new();
        if autosquash {
            execute_args.push("autosquash".to_string());
        }
        self.execute_job(
            &repo_dir,
            "rebase.execute",
            JobLock::RefsWrite,
            execute_args,
            false,
        )?;
        self.store
            .set_active_view(Some(PanelKind::Branches.view_id().to_string()));
        Ok(format!(
            "interactive rebase started from {base_ref}{}",
            if autosquash { " with autosquash" } else { "" }
        ))
    }

    fn select_target(
        &mut self,
        target: SelectionTarget,
        value: &str,
    ) -> Result<String, UserFacingError> {
        match target {
            SelectionTarget::File => {
                let repo_dir = self.require_repo_dir()?;
                let path = normalize_repo_path(&repo_dir, value);
                self.store.update_selected_paths(vec![path.clone()]);
                self.store
                    .set_active_view(Some(PanelKind::Status.view_id().to_string()));
                self.refresh_selected_file_diff()?;
                Ok(format!("selected file {path}"))
            }
            SelectionTarget::Commit => {
                self.store
                    .set_active_view(Some(PanelKind::History.view_id().to_string()));
                self.execute_in_open_repo("history.select_commit", vec![value.to_string()], true)?;
                Ok(format!("selected commit {value}"))
            }
            SelectionTarget::Branch => {
                if self.repo_dir.is_some() && self.store.snapshot().branches.branches.is_empty() {
                    self.execute_in_open_repo("refs.refresh", Vec::new(), false)?;
                }
                if !self.store.snapshot().branches.branches.is_empty()
                    && !self
                        .store
                        .snapshot()
                        .branches
                        .branches
                        .iter()
                        .any(|branch| branch.name == value)
                {
                    return Err(invalid_input_error(
                        "branch is not present in current refs view",
                    ));
                }
                self.store.update_selected_branch(Some(value.to_string()));
                self.store
                    .set_active_view(Some(PanelKind::Branches.view_id().to_string()));
                Ok(format!("selected branch {value}"))
            }
            SelectionTarget::Plugin => {
                let installed = self.sync_plugin_inventory()?;
                if !installed
                    .iter()
                    .any(|plugin| plugin.manifest.plugin_id == value)
                {
                    return Err(invalid_input_error(
                        "plugin is not installed in current plugins root",
                    ));
                }
                self.store.update_selected_plugin(Some(value.to_string()));
                self.store
                    .set_active_view(Some(PanelKind::Diagnostics.view_id().to_string()));
                Ok(format!("selected plugin {value}"))
            }
        }
    }

    fn refresh(&mut self) -> Result<String, UserFacingError> {
        let repo_dir = self.require_repo_dir()?;
        self.execute_job(
            &repo_dir,
            "status.refresh",
            JobLock::Read,
            Vec::new(),
            false,
        )?;
        self.execute_job(&repo_dir, "refs.refresh", JobLock::Read, Vec::new(), false)?;

        if let Some(replayable) = self.last_replayable.clone() {
            self.run_replayable(replayable)?;
        } else if self.store.snapshot().active_view.as_deref() == Some("history.panel") {
            self.execute_job(
                &repo_dir,
                "history.page",
                JobLock::Read,
                vec!["0".to_string(), "20".to_string()],
                true,
            )?;
        } else if !self.store.snapshot().selection.selected_paths.is_empty() {
            let _ = self.refresh_selected_file_diff();
        }

        Ok("refreshed current context".to_string())
    }

    fn run_operational_op(
        &mut self,
        target: &str,
        args: &[String],
    ) -> Result<String, UserFacingError> {
        let repo_root = operations::workspace_root();
        let detail = match target {
            "ops.check_deps" => operations::check_dependency_guards(&repo_root),
            "ops.dev_check" => operations::run_dev_check(&repo_root),
            "release.notes" => {
                let out_file = args
                    .first()
                    .map(|raw| resolve_path(&repo_root, raw))
                    .unwrap_or_else(|| repo_root.join("target/tmp/release-notes.md"));
                let channel = args.get(1).map(String::as_str).unwrap_or("local");
                operations::generate_release_notes(&repo_root, &out_file, channel)
            }
            "release.sign" => {
                let artifact_dir = args
                    .first()
                    .map(|raw| resolve_path(&repo_root, raw))
                    .unwrap_or_else(|| repo_root.join("target/tmp/local-package"));
                operations::sign_artifacts(&artifact_dir)
            }
            "release.package_local" => {
                let out_dir = args
                    .first()
                    .map(|raw| resolve_path(&repo_root, raw))
                    .unwrap_or_else(|| repo_root.join("target/tmp/local-package"));
                let channel = args.get(1).cloned().unwrap_or_else(|| "local".to_string());
                let rollback_from = args
                    .get(2)
                    .cloned()
                    .unwrap_or_else(|| "last-stable".to_string());
                operations::package_local(
                    &repo_root,
                    &operations::LocalPackageOptions {
                        out_dir,
                        channel,
                        rollback_from,
                    },
                )
            }
            "release.package" => {
                let out_dir = args
                    .first()
                    .map(|raw| resolve_path(&repo_root, raw))
                    .unwrap_or_else(|| repo_root.join("target/tmp/release-package"));
                let channel = args.get(1).cloned().unwrap_or_else(|| "stable".to_string());
                let rollback_from = args
                    .get(2)
                    .cloned()
                    .unwrap_or_else(|| "last-stable".to_string());
                operations::package_release(
                    &repo_root,
                    &operations::ReleasePackageOptions {
                        out_dir,
                        channel,
                        rollback_from,
                    },
                )
                .map(|result| {
                    format!(
                        "release package directory: {}\nrelease archive: {}",
                        result.out_dir.display(),
                        result.archive_path.display()
                    )
                })
            }
            "release.verify" => {
                let out_dir = args
                    .first()
                    .map(|raw| resolve_path(&repo_root, raw))
                    .unwrap_or_else(|| repo_root.join("target/tmp/sprint24-package"));
                let channel = args.get(1).cloned().unwrap_or_else(|| "stable".to_string());
                let rollback_from = args
                    .get(2)
                    .cloned()
                    .unwrap_or_else(|| "last-stable".to_string());
                operations::verify_release(
                    &repo_root,
                    &operations::ReleasePackageOptions {
                        out_dir,
                        channel,
                        rollback_from,
                    },
                )
            }
            "verify.sprint22" => operations::verify_sprint22(&repo_root),
            "verify.sprint23" => {
                let out_dir = args
                    .first()
                    .map(|raw| resolve_path(&repo_root, raw))
                    .unwrap_or_else(|| repo_root.join("target/tmp/sprint23-package-check"));
                operations::verify_sprint23(&repo_root, &out_dir)
            }
            "verify.sprint24" => {
                let out_dir = args
                    .first()
                    .map(|raw| resolve_path(&repo_root, raw))
                    .unwrap_or_else(|| repo_root.join("target/tmp/sprint24-package"));
                let channel = args.get(1).cloned().unwrap_or_else(|| "stable".to_string());
                let rollback_from = args
                    .get(2)
                    .cloned()
                    .unwrap_or_else(|| "last-stable".to_string());
                operations::verify_sprint24(
                    &repo_root,
                    &operations::ReleasePackageOptions {
                        out_dir,
                        channel,
                        rollback_from,
                    },
                )
            }
            _ => Err(format!("unsupported operational op `{target}`")),
        }
        .map_err(translate_operational_error)?;

        self.store
            .set_active_view(Some(PanelKind::Diagnostics.view_id().to_string()));
        self.store
            .update_diff(render_text_diff(&format!("ops:{target}"), detail.clone()));
        Ok(detail.lines().next().unwrap_or(target).to_string())
    }

    fn run_workspace_op(
        &mut self,
        target: &str,
        args: &[String],
    ) -> Result<String, UserFacingError> {
        let mut state = self.store.snapshot().workspace.clone();
        match target {
            "workspace.create" => {
                let name = args
                    .first()
                    .cloned()
                    .unwrap_or_else(|| "Default Workspace".to_string());
                let id = unique_workspace_id(&state, &name);
                state.workspaces.push(Workspace {
                    id: id.clone(),
                    name: name.clone(),
                    repos: Vec::new(),
                    groups: Vec::new(),
                    last_opened_ms: current_millis(),
                });
                state.active_workspace_id = Some(id);
                state.active_repo_id = None;
                self.store.update_workspace_state(state);
                self.persist_workspace_state()?;
                Ok(format!("created workspace {name}"))
            }
            "workspace.add_repo" => {
                let raw_path = args.first().ok_or_else(|| {
                    invalid_input_error(
                        "workspace.add_repo requires repository path: `run workspace.add_repo <path> [group]`",
                    )
                })?;
                let repo_path = resolve_path(&self.config.cwd, raw_path);
                let repo = git_service::repo_open(&repo_path).map_err(|err| {
                    UserFacingError::with_category(
                        "Workspace repo failed",
                        "Could not open repository for workspace.",
                        Some(format!("{err:?}")),
                        ErrorCategory::Git,
                    )
                })?;
                let root = PathBuf::from(repo.root);
                let repo_record = workspace_repo_record(&root, args.get(1).cloned())?;
                let workspace_idx = ensure_active_workspace(&mut state);
                let workspace = &mut state.workspaces[workspace_idx];
                if !workspace
                    .repos
                    .iter()
                    .any(|item| item.repo_id == repo_record.repo_id)
                {
                    workspace.repos.push(repo_record.clone());
                }
                state.active_repo_id = Some(repo_record.repo_id.clone());
                self.store.update_workspace_state(state);
                self.persist_workspace_state()?;
                Ok(format!("added workspace repo {}", root.display()))
            }
            "workspace.remove_repo" => {
                let target_repo = args.first().ok_or_else(|| {
                    invalid_input_error("workspace.remove_repo requires repo id or path")
                })?;
                let workspace_idx = active_workspace_index(&state).ok_or_else(|| {
                    invalid_input_error("no active workspace; create one before removing repos")
                })?;
                let mut workspace = state.workspaces[workspace_idx].clone();
                let before = workspace.repos.len();
                workspace.repos.retain(|repo| {
                    repo.repo_id != *target_repo
                        && repo.path != *target_repo
                        && repo.display_name != *target_repo
                });
                if before == workspace.repos.len() {
                    return Err(invalid_input_error(
                        "workspace repo was not found by id, path, or display name",
                    ));
                }
                if state
                    .active_repo_id
                    .as_ref()
                    .is_some_and(|id| !workspace.repos.iter().any(|repo| &repo.repo_id == id))
                {
                    state.active_repo_id = workspace.repos.first().map(|repo| repo.repo_id.clone());
                }
                state.workspaces[workspace_idx] = workspace;
                self.store.update_workspace_state(state);
                self.persist_workspace_state()?;
                Ok(format!("removed workspace repo {target_repo}"))
            }
            "workspace.switch" => {
                let workspace_id = args.first().cloned();
                let idx = workspace_id
                    .as_ref()
                    .and_then(|value| find_workspace_index(&state, value))
                    .or_else(|| (!state.workspaces.is_empty()).then_some(0))
                    .ok_or_else(|| invalid_input_error("no workspace is available"))?;
                state.workspaces[idx].last_opened_ms = current_millis();
                state.active_workspace_id = Some(state.workspaces[idx].id.clone());
                state.active_repo_id = state.workspaces[idx]
                    .repos
                    .first()
                    .map(|repo| repo.repo_id.clone());
                let name = state.workspaces[idx].name.clone();
                self.store.update_workspace_state(state);
                self.persist_workspace_state()?;
                Ok(format!("workspace -> {name}"))
            }
            "workspace.switch_repo" => {
                let target_repo = args.first().ok_or_else(|| {
                    invalid_input_error(
                        "workspace.switch_repo requires repo id, path, or display name",
                    )
                })?;
                let (workspace_idx, repo) = find_workspace_repo(&state, target_repo)
                    .ok_or_else(|| invalid_input_error("workspace repo was not found"))?;
                state.active_workspace_id = Some(state.workspaces[workspace_idx].id.clone());
                state.active_repo_id = Some(repo.repo_id.clone());
                self.store.update_workspace_state(state);
                self.persist_workspace_state()?;
                let opened = self.open_repo(&repo.path)?;
                Ok(format!("workspace repo -> {}; {opened}", repo.display_name))
            }
            "workspace.refresh_all" | "workspace.fetch_all" => {
                let workspace_idx = active_workspace_index(&state).ok_or_else(|| {
                    invalid_input_error("no active workspace; create one and add repos first")
                })?;
                let mut workspace = state.workspaces[workspace_idx].clone();
                let mut results = Vec::new();
                for repo in &mut workspace.repos {
                    if target == "workspace.fetch_all" {
                        let fetch = git_service::fetch_all(Path::new(&repo.path));
                        results.push(WorkspaceJobResult {
                            repo_id: repo.repo_id.clone(),
                            op: "fetch_all".to_string(),
                            success: fetch.is_ok(),
                            message: fetch
                                .map(|text| {
                                    if text.is_empty() {
                                        "fetch --all completed".to_string()
                                    } else {
                                        text
                                    }
                                })
                                .unwrap_or_else(|err| format!("{err:?}")),
                        });
                    }
                    match summarize_workspace_repo(Path::new(&repo.path)) {
                        Ok((status, branch)) => {
                            repo.status_summary = status;
                            repo.branch_summary = branch;
                            results.push(WorkspaceJobResult {
                                repo_id: repo.repo_id.clone(),
                                op: "refresh".to_string(),
                                success: true,
                                message: "refreshed".to_string(),
                            });
                        }
                        Err(message) => {
                            repo.status_summary.last_error = Some(message.clone());
                            results.push(WorkspaceJobResult {
                                repo_id: repo.repo_id.clone(),
                                op: "refresh".to_string(),
                                success: false,
                                message,
                            });
                        }
                    }
                }
                state.workspaces[workspace_idx] = workspace;
                state.last_results = results;
                self.store.update_workspace_state(state);
                self.persist_workspace_state()?;
                Ok(format!(
                    "{} workspace repos",
                    if target == "workspace.fetch_all" {
                        "fetched"
                    } else {
                        "refreshed"
                    }
                ))
            }
            "workspace.persist" => {
                let out_file = args
                    .first()
                    .map(|raw| resolve_path(&self.config.cwd, raw))
                    .unwrap_or_else(|| workspace_store_path_for(&self.config.cwd));
                self.persist_workspace_state_to(&out_file)?;
                Ok(format!(
                    "persisted workspace state to {}",
                    out_file.display()
                ))
            }
            "workspace.restore" => {
                let in_file = args
                    .first()
                    .map(|raw| resolve_path(&self.config.cwd, raw))
                    .unwrap_or_else(|| workspace_store_path_for(&self.config.cwd));
                self.store.restore_workspaces(&in_file).map_err(|err| {
                    UserFacingError::with_category(
                        "Workspace restore failed",
                        "Could not restore workspace state.",
                        Some(err),
                        ErrorCategory::System,
                    )
                })?;
                Ok(format!(
                    "restored workspace state from {}",
                    in_file.display()
                ))
            }
            _ => Err(invalid_input_error("unsupported workspace operation")),
        }
    }

    fn run_auth_op(&mut self, target: &str, args: &[String]) -> Result<String, UserFacingError> {
        match target {
            "auth.status" => {
                self.refresh_auth_state_for_current_context();
                let summary = render_auth_summary(&self.store.snapshot().remotes.auth);
                self.store
                    .update_diff(render_text_diff("auth:status", summary.clone()));
                Ok(summary)
            }
            "auth.login" => {
                let host = args.first().ok_or_else(|| {
                    invalid_input_error(
                        "auth.login requires host, username, and token: `run auth.login <host> <username> <token> [provider]`",
                    )
                })?;
                let username = args.get(1).ok_or_else(|| {
                    invalid_input_error(
                        "auth.login requires username: `run auth.login <host> <username> <token> [provider]`",
                    )
                })?;
                let token = args.get(2).ok_or_else(|| {
                    invalid_input_error(
                        "auth.login requires token: `run auth.login <host> <username> <token> [provider]`",
                    )
                })?;
                let provider = args
                    .get(3)
                    .and_then(|raw| parse_provider_kind(raw))
                    .or_else(|| provider_from_host(host));
                let record = self
                    .credential_vault()
                    .store_token(host, username, provider, token)
                    .map_err(|err| {
                        UserFacingError::with_category(
                            "Credential storage failed",
                            "Could not save the token in the host credential store.",
                            Some(err),
                            ErrorCategory::System,
                        )
                    })?;

                if let Some(repo_dir) = self.repo_dir.clone() {
                    let _ = self.seed_git_credential_for(
                        &repo_dir,
                        &record.host,
                        record.username.as_deref(),
                    );
                }
                self.refresh_auth_state_for_current_context();
                Ok(format!(
                    "stored credential for {} as {}",
                    record.host,
                    record.username.as_deref().unwrap_or("<unknown>")
                ))
            }
            "auth.logout" => {
                let host = args.first().ok_or_else(|| {
                    invalid_input_error(
                        "auth.logout requires host: `run auth.logout <host> [username]`",
                    )
                })?;
                let username = args.get(1).map(String::as_str);
                let removed = self
                    .credential_vault()
                    .delete_token(host, username)
                    .map_err(|err| {
                        UserFacingError::with_category(
                            "Credential removal failed",
                            "Could not remove the stored token.",
                            Some(err),
                            ErrorCategory::System,
                        )
                    })?;
                if let Some(repo_dir) = self.repo_dir.clone() {
                    for account in &removed {
                        let _ = git_service::credential_reject(
                            &repo_dir,
                            "https",
                            host,
                            Some(account.as_str()),
                        );
                    }
                }
                self.refresh_auth_state_for_current_context();
                Ok(format!(
                    "removed {} credential(s) for {host}",
                    removed.len()
                ))
            }
            "auth.seed_git" => {
                let host = args.first().ok_or_else(|| {
                    invalid_input_error(
                        "auth.seed_git requires host: `run auth.seed_git <host> [username]`",
                    )
                })?;
                let username = args.get(1).map(String::as_str);
                let repo_dir = self.require_repo_dir()?;
                let credential = self.seed_git_credential_for(&repo_dir, host, username)?;
                self.refresh_auth_state_for_current_context();
                Ok(format!(
                    "approved Git credential for {} as {}",
                    credential.host, credential.username
                ))
            }
            _ => Err(invalid_input_error("unsupported auth operation")),
        }
    }

    fn credential_vault(&self) -> CredentialVault {
        CredentialVault::with_overrides(
            &self.config.cwd,
            self.config.auth_metadata_path.as_deref(),
            self.config.auth_file_store.as_deref(),
        )
    }

    fn seed_git_credential_for(
        &self,
        repo_dir: &Path,
        host: &str,
        username: Option<&str>,
    ) -> Result<StoredCredential, UserFacingError> {
        let credential = self
            .credential_vault()
            .token_for_host(host, username)
            .map_err(|err| {
                UserFacingError::with_category(
                    "Git credential approval failed",
                    "No stored token is available for this host.",
                    Some(err),
                    ErrorCategory::Validation,
                )
            })?;
        git_service::credential_approve(
            repo_dir,
            &git_service::GitCredentialInput {
                protocol: "https".to_string(),
                host: credential.host.clone(),
                username: credential.username.clone(),
                password: credential.token.clone(),
            },
        )
        .map_err(|err| {
            UserFacingError::with_category(
                "Git credential approval failed",
                "Could not pass the stored token to Git's credential helper.",
                Some(format!("{err:?}")),
                ErrorCategory::Git,
            )
        })?;
        Ok(credential)
    }

    fn prepare_https_credentials(
        &mut self,
        repo_dir: &Path,
        op: &str,
        args: &[String],
    ) -> Result<(), UserFacingError> {
        let Some(host) = self.https_host_for_remote_op(op, args) else {
            return Ok(());
        };
        match self.seed_git_credential_for(repo_dir, &host, None) {
            Ok(_) => Ok(()),
            Err(error) if error.title == "Git credential approval failed" => {
                let mut remotes = self.store.snapshot().remotes.clone();
                remotes.auth.last_error = Some(format!(
                    "No stored HTTPS token for {host}. Use auth.login before fetch/pull/push."
                ));
                self.store.update_remote_state(remotes);
                Ok(())
            }
            Err(error) => Err(error),
        }
    }

    fn https_host_for_remote_op(&self, op: &str, args: &[String]) -> Option<String> {
        if !matches!(
            op,
            "remote.fetch"
                | "remote.fetch_all"
                | "remote.pull"
                | "remote.push"
                | "remote.push_set_upstream"
                | "remote.push_force_with_lease"
        ) {
            return None;
        }

        let snapshot = self.store.snapshot();
        let remote_name = args
            .first()
            .filter(|value| !value.trim().is_empty())
            .cloned()
            .or_else(|| {
                snapshot
                    .remotes
                    .upstream
                    .as_ref()
                    .and_then(|upstream| upstream.upstream.as_deref())
                    .and_then(|upstream| {
                        upstream
                            .split_once('/')
                            .map(|(remote, _)| remote.to_string())
                    })
            });
        let remote = remote_name
            .as_deref()
            .and_then(|name| {
                snapshot
                    .remotes
                    .remotes
                    .iter()
                    .find(|remote| remote.name == name)
            })
            .or_else(|| snapshot.remotes.remotes.first())?;
        let url = remote.push_url.as_deref().or(remote.fetch_url.as_deref())?;
        https_host_from_url(url)
    }

    fn refresh_auth_state_for_current_context(&mut self) {
        let cwd = self
            .repo_dir
            .clone()
            .unwrap_or_else(|| self.config.cwd.clone());
        self.refresh_auth_state(&cwd);
    }

    fn refresh_auth_state(&mut self, cwd: &Path) {
        let git_auth = git_service::auth_status(cwd).unwrap_or_default();
        let mut last_error = None;
        let accounts = match self.credential_vault().list_accounts() {
            Ok(accounts) => accounts,
            Err(err) => {
                last_error = Some(err);
                Vec::new()
            }
        };
        let mut remote_state = self.store.snapshot().remotes.clone();
        remote_state.auth = AuthStatus {
            ssh_agent_available: git_auth.ssh_agent_available,
            https_helper_configured: git_auth.https_helper_configured,
            accounts,
            last_error,
        };
        self.store.update_remote_state(remote_state);
    }

    fn run_pr_op(&mut self, target: &str, args: &[String]) -> Result<String, UserFacingError> {
        let repo_dir = self.require_repo_dir()?;
        match target {
            "pr.detect_provider" => {
                let (provider, _) = self.detect_provider_for_repo(&repo_dir)?;
                self.store
                    .update_pull_request_state(PullRequestStateSnapshot {
                        detected_provider: Some(provider.clone()),
                        ..self.store.snapshot().pull_requests.clone()
                    });
                Ok(format!(
                    "detected {:?} provider at {}",
                    provider.provider, provider.web_url
                ))
            }
            "pr.list" => {
                let (provider, _) = self.detect_provider_for_repo(&repo_dir)?;
                let current_branch = self
                    .store
                    .snapshot()
                    .repo
                    .as_ref()
                    .and_then(|repo| repo.head.clone())
                    .or_else(|| {
                        git_service::upstream_status(&repo_dir)
                            .ok()
                            .and_then(|s| s.current_branch)
                    });
                let token = match self.credential_vault().token_for_host(&provider.host, None) {
                    Ok(credential) => credential.token,
                    Err(err) => {
                        let recovery = format!(
                            "No stored provider token for {}. Store one with auth.login, then retry pr.list. ({err})",
                            provider.host
                        );
                        self.store
                            .update_pull_request_state(PullRequestStateSnapshot {
                                detected_provider: Some(provider),
                                pull_requests: Vec::new(),
                                current_branch_pr: None,
                                last_error: Some(recovery.clone()),
                            });
                        return Ok(recovery);
                    }
                };
                let pull_requests = list_pull_requests(
                    &provider,
                    &token,
                    &ProviderApiConfig {
                        github_api_base: self.config.github_api_base.clone(),
                        gitlab_api_base: self.config.gitlab_api_base.clone(),
                    },
                )
                .map_err(|err| {
                    UserFacingError::with_category(
                        "Provider API request failed",
                        "Could not list pull requests from the hosting provider.",
                        Some(err.message),
                        ErrorCategory::System,
                    )
                })?;
                let current = current_branch.as_deref().and_then(|head| {
                    pull_requests
                        .iter()
                        .find(|pr| pr.source_branch == head)
                        .cloned()
                });
                self.store
                    .update_pull_request_state(PullRequestStateSnapshot {
                        detected_provider: Some(provider),
                        pull_requests,
                        current_branch_pr: current,
                        last_error: None,
                    });
                Ok("listed pull requests from provider API".to_string())
            }
            "pr.create_url" => {
                let (provider, _) = self.detect_provider_for_repo(&repo_dir)?;
                let base = args.first().map(String::as_str).unwrap_or("main");
                let head = args
                    .get(1)
                    .cloned()
                    .or_else(|| {
                        self.store
                            .snapshot()
                            .repo
                            .as_ref()
                            .and_then(|repo| repo.head.clone())
                    })
                    .ok_or_else(|| {
                        invalid_input_error("pr.create_url requires head branch when detached")
                    })?;
                let title = args.get(2).map(|_| args[2..].join(" "));
                let url = create_pull_request_url(&provider, base, &head, title.as_deref());
                let summary = pull_request_summary_for_url(&provider, base, &head, url.clone());
                self.store
                    .update_pull_request_state(PullRequestStateSnapshot {
                        detected_provider: Some(provider),
                        pull_requests: vec![summary.clone()],
                        current_branch_pr: Some(summary),
                        last_error: None,
                    });
                self.store
                    .update_diff(render_text_diff("pr:create_url", url.clone()));
                Ok(url)
            }
            "pr.open" => {
                let url = args
                    .first()
                    .cloned()
                    .or_else(|| {
                        self.store
                            .snapshot()
                            .pull_requests
                            .current_branch_pr
                            .as_ref()
                            .and_then(|pr| pr.web_url.clone())
                    })
                    .ok_or_else(|| {
                        invalid_input_error("pr.open requires a URL or current PR state")
                    })?;
                self.store
                    .update_diff(render_text_diff("pr:open", format!("open {url}")));
                Ok(format!("PR URL: {url}"))
            }
            "pr.checkout" => {
                let number = args
                    .first()
                    .and_then(|value| value.parse::<u64>().ok())
                    .ok_or_else(|| invalid_input_error("pr.checkout requires numeric PR/MR id"))?;
                let local_branch = args
                    .get(1)
                    .cloned()
                    .unwrap_or_else(|| format!("pr/{number}"));
                let (provider, remote) = self.detect_provider_for_repo(&repo_dir)?;
                git_service::checkout_pull_request(
                    &repo_dir,
                    map_provider_kind_for_git(&provider.provider),
                    &remote,
                    number,
                    &local_branch,
                )
                .map_err(|err| {
                    UserFacingError::with_category(
                        "PR checkout failed",
                        "Could not fetch and checkout pull request branch.",
                        Some(format!("{err:?}")),
                        ErrorCategory::Git,
                    )
                })?;
                self.execute_in_open_repo("status.refresh", Vec::new(), false)?;
                Ok(format!("checked out PR/MR {number} as {local_branch}"))
            }
            _ => Err(invalid_input_error("unsupported PR operation")),
        }
    }

    fn run_stack_op(&mut self, target: &str, args: &[String]) -> Result<String, UserFacingError> {
        let repo_dir = self.require_repo_dir()?;
        match target {
            "stack.create" => {
                let name = args.first().cloned().ok_or_else(|| {
                    invalid_input_error("stack.create requires name, base, and branches")
                })?;
                let base = args.get(1).cloned().ok_or_else(|| {
                    invalid_input_error("stack.create requires base ref in args[1]")
                })?;
                let branches = args.get(2..).unwrap_or(&[]);
                if branches.is_empty() {
                    return Err(invalid_input_error(
                        "stack.create requires at least one branch after the base",
                    ));
                }
                let mut entries = Vec::new();
                let mut parent = base;
                for branch in branches {
                    entries.push(build_stack_entry(&repo_dir, &parent, branch)?);
                    parent = branch.clone();
                }
                let id = stack_id_for(&name);
                let mut state = self.store.snapshot().branch_stacks.clone();
                upsert_stack(
                    &mut state,
                    BranchStack {
                        id: id.clone(),
                        name: name.clone(),
                        entries,
                    },
                );
                state.active_stack_id = Some(id);
                state.last_error = None;
                self.store.update_branch_stack_state(state);
                Ok(format!("created branch stack {name}"))
            }
            "stack.detect" => {
                let base = args
                    .first()
                    .cloned()
                    .or_else(|| {
                        self.store
                            .snapshot()
                            .repo
                            .as_ref()
                            .and_then(|repo| repo.head.clone())
                    })
                    .unwrap_or_else(|| "main".to_string());
                let mut entries = Vec::new();
                for branch in git_service::list_local_branches(&repo_dir).map_err(|err| {
                    UserFacingError::with_category(
                        "Stack detection failed",
                        "Could not list local branches.",
                        Some(format!("{err:?}")),
                        ErrorCategory::Git,
                    )
                })? {
                    if branch.name == base {
                        continue;
                    }
                    if let Ok(entry) = build_stack_entry(&repo_dir, &base, &branch.name)
                        && entry.ahead > 0
                    {
                        entries.push(entry);
                    }
                }
                entries.sort_by(|left, right| left.branch.cmp(&right.branch));
                let name = format!("Detected from {base}");
                let id = stack_id_for(&name);
                let mut state = self.store.snapshot().branch_stacks.clone();
                upsert_stack(
                    &mut state,
                    BranchStack {
                        id: id.clone(),
                        name: name.clone(),
                        entries,
                    },
                );
                state.active_stack_id = Some(id);
                state.last_error = None;
                self.store.update_branch_stack_state(state);
                Ok(format!("detected branch stack {name}"))
            }
            "stack.restack" => {
                if !git_service::worktree_is_clean(&repo_dir).unwrap_or(false) {
                    return Err(UserFacingError::with_category(
                        "Restack blocked",
                        "Working tree must be clean before restacking.",
                        None,
                        ErrorCategory::Validation,
                    ));
                }
                let state = self.store.snapshot().branch_stacks.clone();
                let stack_id = args
                    .first()
                    .cloned()
                    .or_else(|| state.active_stack_id.clone())
                    .ok_or_else(|| invalid_input_error("stack.restack requires stack id"))?;
                let stack = state
                    .stacks
                    .iter()
                    .find(|stack| stack.id == stack_id || stack.name == stack_id)
                    .cloned()
                    .ok_or_else(|| invalid_input_error("branch stack was not found"))?;
                for entry in &stack.entries {
                    self.execute_in_open_repo(
                        "stack.restack_branch",
                        vec![entry.branch.clone(), entry.base_branch.clone()],
                        false,
                    )?;
                }
                let refreshed_entries = stack
                    .entries
                    .iter()
                    .map(|entry| build_stack_entry(&repo_dir, &entry.base_branch, &entry.branch))
                    .collect::<Result<Vec<_>, _>>()?;
                let mut refreshed_state = self.store.snapshot().branch_stacks.clone();
                upsert_stack(
                    &mut refreshed_state,
                    BranchStack {
                        id: stack.id.clone(),
                        name: stack.name.clone(),
                        entries: refreshed_entries,
                    },
                );
                refreshed_state.active_stack_id = Some(stack.id.clone());
                refreshed_state.last_error = None;
                self.store.update_branch_stack_state(refreshed_state);
                Ok(format!("restacked branch stack {}", stack.name))
            }
            _ => Err(invalid_input_error("unsupported stack operation")),
        }
    }

    fn persist_workspace_state(&self) -> Result<(), UserFacingError> {
        self.persist_workspace_state_to(&workspace_store_path_for(&self.config.cwd))
    }

    fn persist_workspace_state_to(&self, path: &Path) -> Result<(), UserFacingError> {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).map_err(|err| {
                UserFacingError::with_category(
                    "Workspace persist failed",
                    "Could not create workspace state directory.",
                    Some(err.to_string()),
                    ErrorCategory::System,
                )
            })?;
        }
        self.store.persist_workspaces(path).map_err(|err| {
            UserFacingError::with_category(
                "Workspace persist failed",
                "Could not persist workspace state.",
                Some(err),
                ErrorCategory::System,
            )
        })
    }

    fn detect_provider_for_repo(
        &self,
        repo_dir: &Path,
    ) -> Result<(ProviderRepository, String), UserFacingError> {
        let remotes = git_service::remote_list(repo_dir).map_err(|err| {
            UserFacingError::with_category(
                "Provider detection failed",
                "Could not list remotes.",
                Some(format!("{err:?}")),
                ErrorCategory::Git,
            )
        })?;
        remotes
            .iter()
            .filter_map(|remote| {
                remote
                    .fetch_url
                    .as_ref()
                    .or(remote.push_url.as_ref())
                    .and_then(|url| git_service::detect_remote_provider(url))
                    .map(|provider| (map_provider_repository(provider), remote.name.clone()))
            })
            .find(|(provider, _)| provider.provider != state_store::ProviderKind::Unknown)
            .or_else(|| {
                remotes.iter().find_map(|remote| {
                    remote
                        .fetch_url
                        .as_ref()
                        .or(remote.push_url.as_ref())
                        .and_then(|url| git_service::detect_remote_provider(url))
                        .map(|provider| (map_provider_repository(provider), remote.name.clone()))
                })
            })
            .ok_or_else(|| {
                invalid_input_error("no GitHub/GitLab-style remote URL detected for this repo")
            })
    }

    fn run_plugin_op(&mut self, op: PluginOp, confirmed: bool) -> Result<String, UserFacingError> {
        let action_id = match &op {
            PluginOp::List => "plugin.list",
            PluginOp::Discover { .. } => "plugin.discover",
            PluginOp::Marketplace { .. } => "plugin.marketplace",
            PluginOp::Install { .. } => "plugin.install",
            PluginOp::InstallRegistry { .. } => "plugin.install_registry",
            PluginOp::Update { .. } => "plugin.update",
            PluginOp::Enable { .. } => "plugin.enable",
            PluginOp::Disable { .. } => "plugin.disable",
            PluginOp::Remove { .. } => "plugin.remove",
        };
        self.ensure_confirmation(action_id, &[], confirmed)?;

        match op {
            PluginOp::List => {
                let installed = self.sync_plugin_inventory()?;
                self.store
                    .set_active_view(Some("diagnostics.panel".to_string()));
                self.store.update_diff(render_text_diff(
                    "plugin:list",
                    render_plugin_list(&installed, &self.config.plugins_root),
                ));
                self.last_replayable = Some(ReplayableRun::Run {
                    target: "plugin.list".to_string(),
                    args: Vec::new(),
                });
                Ok(format!(
                    "listed plugins from {}",
                    self.config.plugins_root.display()
                ))
            }
            PluginOp::Discover { registry_path } => {
                let registry = self.resolve_plugin_registry_path(registry_path.as_deref());
                let discovered =
                    discover_local_plugins(&registry).map_err(translate_plugin_manager_error)?;
                self.store
                    .set_active_view(Some("diagnostics.panel".to_string()));
                self.store.update_diff(render_text_diff(
                    "plugin:discover",
                    render_discovered_plugin_list(&discovered, &registry),
                ));
                self.last_replayable = Some(ReplayableRun::Run {
                    target: "plugin.discover".to_string(),
                    args: registry_path.into_iter().collect(),
                });
                Ok(format!("discovered plugins from {}", registry.display()))
            }
            PluginOp::Marketplace { registry_path } => {
                let registry = self.resolve_plugin_registry_path(registry_path.as_deref());
                let discovered =
                    discover_local_plugins(&registry).map_err(translate_plugin_manager_error)?;
                let installed = self.sync_plugin_inventory()?;
                self.store
                    .update_plugin_security(map_plugin_security_records_with_updates(
                        &installed,
                        &self.actions,
                        &self.config.plugins_root,
                        &discovered,
                    ));
                self.store
                    .set_active_view(Some("diagnostics.panel".to_string()));
                self.store.update_diff(render_text_diff(
                    "plugin:marketplace",
                    render_plugin_marketplace_list(&discovered, &installed, &registry),
                ));
                self.last_replayable = Some(ReplayableRun::Run {
                    target: "plugin.marketplace".to_string(),
                    args: registry_path.into_iter().collect(),
                });
                Ok(format!(
                    "loaded plugin marketplace from {}",
                    registry.display()
                ))
            }
            PluginOp::Install { package_dir } => {
                let path = resolve_path(&self.config.cwd, &package_dir);
                let installed = install_local_plugin(&path, &self.config.plugins_root)
                    .map_err(translate_plugin_manager_error)?;
                self.store
                    .update_selected_plugin(Some(installed.manifest.plugin_id.clone()));
                self.sync_plugin_inventory()?;
                self.store
                    .set_active_view(Some("diagnostics.panel".to_string()));
                self.store.update_diff(render_text_diff(
                    "plugin:install",
                    format!(
                        "installed plugin {}\nversion: {}\nenabled: {}\npermissions: {}",
                        installed.manifest.plugin_id,
                        installed.manifest.version,
                        installed.enabled,
                        installed.manifest.permissions.join(", ")
                    ),
                ));
                self.last_replayable = Some(ReplayableRun::Run {
                    target: "plugin.list".to_string(),
                    args: Vec::new(),
                });
                Ok(format!("installed plugin {}", installed.manifest.plugin_id))
            }
            PluginOp::InstallRegistry {
                plugin_id,
                registry_path,
            } => {
                let registry = self.resolve_plugin_registry_path(registry_path.as_deref());
                let installed =
                    install_registry_plugin(&registry, &self.config.plugins_root, &plugin_id)
                        .map_err(translate_plugin_manager_error)?;
                self.store
                    .update_selected_plugin(Some(installed.manifest.plugin_id.clone()));
                self.sync_plugin_inventory()?;
                self.store
                    .set_active_view(Some("diagnostics.panel".to_string()));
                self.store.update_diff(render_text_diff(
                    "plugin:install_registry",
                    format!(
                        "installed registry plugin {}\nversion: {}\nregistry: {}\nenabled: {}",
                        installed.manifest.plugin_id,
                        installed.manifest.version,
                        registry.display(),
                        installed.enabled
                    ),
                ));
                self.last_replayable = Some(ReplayableRun::Run {
                    target: "plugin.list".to_string(),
                    args: Vec::new(),
                });
                Ok(format!(
                    "installed registry plugin {}",
                    installed.manifest.plugin_id
                ))
            }
            PluginOp::Update {
                plugin_id,
                registry_path,
            } => {
                let registry = self.resolve_plugin_registry_path(registry_path.as_deref());
                let discovered =
                    discover_local_plugins(&registry).map_err(translate_plugin_manager_error)?;
                let candidate = discovered
                    .iter()
                    .find(|plugin| plugin.manifest.plugin_id == plugin_id)
                    .ok_or_else(|| {
                        UserFacingError::with_category(
                            "Plugin update failed",
                            "Plugin was not found in the marketplace registry.",
                            Some(plugin_id.clone()),
                            ErrorCategory::System,
                        )
                    })?;
                let installed_before = list_installed_plugins(&self.config.plugins_root)
                    .map_err(translate_plugin_manager_error)?;
                if let Some(current) = installed_before
                    .iter()
                    .find(|plugin| plugin.manifest.plugin_id == plugin_id)
                    && current.manifest.version == candidate.manifest.version
                {
                    self.sync_plugin_inventory()?;
                    return Ok(format!(
                        "plugin {plugin_id} is already at {}",
                        candidate.manifest.version
                    ));
                }
                if installed_before
                    .iter()
                    .any(|plugin| plugin.manifest.plugin_id == plugin_id)
                {
                    remove_local_plugin(&self.config.plugins_root, &plugin_id)
                        .map_err(translate_plugin_manager_error)?;
                }
                let installed =
                    install_registry_plugin(&registry, &self.config.plugins_root, &plugin_id)
                        .map_err(translate_plugin_manager_error)?;
                self.store
                    .update_selected_plugin(Some(installed.manifest.plugin_id.clone()));
                self.sync_plugin_inventory()?;
                self.store
                    .set_active_view(Some("diagnostics.panel".to_string()));
                self.store.update_diff(render_text_diff(
                    "plugin:update",
                    format!(
                        "updated plugin {}\nversion: {}\nregistry: {}",
                        installed.manifest.plugin_id,
                        installed.manifest.version,
                        registry.display()
                    ),
                ));
                Ok(format!(
                    "updated plugin {} to {}",
                    installed.manifest.plugin_id, installed.manifest.version
                ))
            }
            PluginOp::Enable { plugin_id } => {
                self.sync_plugin_inventory()?;
                let plugin_id = plugin_id.unwrap_or(self.selected_plugin_id()?);
                let updated = set_plugin_enabled(&self.config.plugins_root, &plugin_id, true)
                    .map_err(translate_plugin_manager_error)?;
                self.store
                    .update_selected_plugin(Some(updated.manifest.plugin_id.clone()));
                self.sync_plugin_inventory()?;
                self.store
                    .set_active_view(Some("diagnostics.panel".to_string()));
                self.store.update_diff(render_text_diff(
                    "plugin:enable",
                    format!(
                        "enabled plugin {}\nversion: {}\ninstall_dir: {}",
                        updated.manifest.plugin_id,
                        updated.manifest.version,
                        updated.install_dir.display()
                    ),
                ));
                self.last_replayable = Some(ReplayableRun::Run {
                    target: "plugin.list".to_string(),
                    args: Vec::new(),
                });
                Ok(format!("enabled plugin {}", updated.manifest.plugin_id))
            }
            PluginOp::Disable { plugin_id } => {
                self.sync_plugin_inventory()?;
                let plugin_id = plugin_id.unwrap_or(self.selected_plugin_id()?);
                let updated = set_plugin_enabled(&self.config.plugins_root, &plugin_id, false)
                    .map_err(translate_plugin_manager_error)?;
                self.store
                    .update_selected_plugin(Some(updated.manifest.plugin_id.clone()));
                self.sync_plugin_inventory()?;
                self.store
                    .set_active_view(Some("diagnostics.panel".to_string()));
                self.store.update_diff(render_text_diff(
                    "plugin:disable",
                    format!(
                        "disabled plugin {}\nversion: {}\ninstall_dir: {}",
                        updated.manifest.plugin_id,
                        updated.manifest.version,
                        updated.install_dir.display()
                    ),
                ));
                self.last_replayable = Some(ReplayableRun::Run {
                    target: "plugin.list".to_string(),
                    args: Vec::new(),
                });
                Ok(format!("disabled plugin {}", updated.manifest.plugin_id))
            }
            PluginOp::Remove { plugin_id } => {
                self.sync_plugin_inventory()?;
                let plugin_id = plugin_id.unwrap_or(self.selected_plugin_id()?);
                remove_local_plugin(&self.config.plugins_root, &plugin_id)
                    .map_err(translate_plugin_manager_error)?;
                if self
                    .store
                    .snapshot()
                    .selection
                    .selected_plugin_id
                    .as_deref()
                    == Some(plugin_id.as_str())
                {
                    self.store.update_selected_plugin(None);
                }
                self.sync_plugin_inventory()?;
                self.store
                    .set_active_view(Some("diagnostics.panel".to_string()));
                self.store.update_diff(render_text_diff(
                    "plugin:remove",
                    format!("removed plugin {plugin_id}"),
                ));
                self.last_replayable = Some(ReplayableRun::Run {
                    target: "plugin.list".to_string(),
                    args: Vec::new(),
                });
                Ok(format!("removed plugin {plugin_id}"))
            }
        }
    }

    fn sync_plugin_inventory(
        &mut self,
    ) -> Result<Vec<plugin_host::InstalledPluginInfo>, UserFacingError> {
        let installed = list_installed_plugins(&self.config.plugins_root)
            .map_err(translate_plugin_manager_error)?;
        self.sync_dynamic_plugin_runtimes(&installed);
        if let Some(selected_plugin_id) = self.store.snapshot().selection.selected_plugin_id.clone()
            && !installed
                .iter()
                .any(|plugin| plugin.manifest.plugin_id == selected_plugin_id)
        {
            self.store.update_selected_plugin(None);
        }
        self.store
            .update_installed_plugins(map_installed_plugins(&installed));
        self.rebuild_catalog_actions();
        self.store
            .update_plugin_security(map_plugin_security_records(
                &installed,
                &self.actions,
                &self.config.plugins_root,
            ));
        Ok(installed)
    }

    fn sync_dynamic_plugin_runtimes(&mut self, installed: &[plugin_host::InstalledPluginInfo]) {
        let installed_ids = installed
            .iter()
            .filter(|plugin| plugin.enabled)
            .map(|plugin| plugin.manifest.plugin_id.clone())
            .collect::<Vec<_>>();
        let to_remove = self
            .dynamic_plugins
            .keys()
            .filter(|plugin_id| !installed_ids.iter().any(|id| id == *plugin_id))
            .cloned()
            .collect::<Vec<_>>();
        for plugin_id in to_remove {
            self.remove_dynamic_plugin_runtime(&plugin_id);
        }

        for plugin in installed {
            if !plugin.enabled {
                self.remove_dynamic_plugin_runtime(&plugin.manifest.plugin_id);
                continue;
            }
            if self
                .dynamic_plugins
                .contains_key(&plugin.manifest.plugin_id)
            {
                self.store.update_plugin_status(
                    &plugin.manifest.plugin_id,
                    state_store::PluginHealth::Ready,
                );
                continue;
            }
            match self.spawn_dynamic_plugin_runtime(plugin) {
                Ok(runtime) => {
                    self.store.update_plugin_status(
                        &plugin.manifest.plugin_id,
                        state_store::PluginHealth::Ready,
                    );
                    self.dynamic_plugins
                        .insert(plugin.manifest.plugin_id.clone(), runtime);
                }
                Err(message) => {
                    self.store.update_plugin_status(
                        &plugin.manifest.plugin_id,
                        state_store::PluginHealth::Unavailable { message },
                    );
                }
            }
        }
    }

    fn spawn_dynamic_plugin_runtime(
        &self,
        plugin: &plugin_host::InstalledPluginInfo,
    ) -> Result<DynamicPluginRuntime, String> {
        let mut process = spawn_installed_plugin_process(plugin).map_err(|err| {
            format!(
                "spawn {} from {} failed: {err:?}",
                plugin.manifest.plugin_id,
                plugin.install_dir.display()
            )
        })?;
        let bootstrapped = bootstrap_plugin_runtime(&mut process).map_err(|err| {
            format!(
                "bootstrap {} from {} failed: {err:?}",
                plugin.manifest.plugin_id,
                plugin.install_dir.display()
            )
        })?;
        Ok(DynamicPluginRuntime {
            process,
            session: bootstrapped.session,
        })
    }

    fn remove_dynamic_plugin_runtime(&mut self, plugin_id: &str) {
        if let Some(mut runtime) = self.dynamic_plugins.remove(plugin_id) {
            let _ = runtime.process.shutdown();
        }
    }

    fn rebuild_catalog_actions(&mut self) {
        let mut actions = build_builtin_catalog_actions();
        for (plugin_id, runtime) in &self.dynamic_plugins {
            push_actions(&mut actions, plugin_id, runtime.session.list_actions());
        }
        self.actions = actions;
    }

    fn find_dynamic_plugin_owner(&self, action_id: &str) -> Option<String> {
        self.dynamic_plugins
            .iter()
            .find_map(|(plugin_id, runtime)| {
                if runtime.session.action_owner(action_id).is_some() {
                    Some(plugin_id.clone())
                } else {
                    None
                }
            })
    }

    fn invoke_dynamic_plugin_action(
        &mut self,
        action_id: &str,
        args: &[String],
    ) -> Result<String, UserFacingError> {
        if !args.is_empty() {
            return Err(invalid_input_error(
                "external plugin actions do not accept CLI args yet; use selection state instead",
            ));
        }
        let owner = self.find_dynamic_plugin_owner(action_id).ok_or_else(|| {
            UserFacingError::with_category(
                "Unsupported operation",
                &format!("Unknown dynamic plugin action `{action_id}`."),
                None,
                ErrorCategory::System,
            )
        })?;
        let runtime = self.dynamic_plugins.get_mut(&owner).ok_or_else(|| {
            UserFacingError::with_category(
                "Plugin unavailable",
                &format!("Plugin `{owner}` is not running."),
                None,
                ErrorCategory::System,
            )
        })?;
        let selection_files = self.store.snapshot().selection.selected_paths.clone();
        let result = invoke_plugin_action(
            &mut runtime.process,
            &mut runtime.session,
            action_id,
            plugin_api::ActionContext { selection_files },
            std::time::Instant::now(),
        )
        .map_err(|err| {
            self.remove_dynamic_plugin_runtime(&owner);
            self.rebuild_catalog_actions();
            self.store.update_plugin_status(
                &owner,
                state_store::PluginHealth::Unavailable {
                    message: format!("runtime invoke failed: {err:?}"),
                },
            );
            UserFacingError::with_category(
                "Plugin invocation failed",
                &format!("Plugin `{owner}` could not handle `{action_id}`."),
                Some(format!("{err:?}")),
                ErrorCategory::System,
            )
        })?;

        let message = result
            .get("message")
            .and_then(serde_json::Value::as_str)
            .map(str::to_string)
            .unwrap_or_else(|| format!("plugin `{owner}` handled `{action_id}`"));
        self.last_replayable = Some(ReplayableRun::Run {
            target: action_id.to_string(),
            args: Vec::new(),
        });
        Ok(message)
    }

    fn resolve_plugin_registry_path(&self, raw: Option<&str>) -> PathBuf {
        raw.map(|value| {
            if value.contains("://") {
                PathBuf::from(value)
            } else {
                resolve_path(&self.config.cwd, value)
            }
        })
        .unwrap_or_else(|| self.config.cwd.join("plugin_registry/registry.json"))
    }

    fn resolve_run_args(
        &self,
        target: &str,
        args: &[String],
    ) -> Result<Vec<String>, UserFacingError> {
        match target {
            "history.page" => {
                if args.is_empty() {
                    Ok(vec!["0".to_string(), "20".to_string()])
                } else if args.len() == 1 {
                    Ok(vec![args[0].clone(), "20".to_string()])
                } else {
                    Ok(args.to_vec())
                }
            }
            "history.select_commit"
            | "history.details"
            | "diff.commit"
            | "cherry_pick.commit"
            | "revert.commit" => {
                if args.is_empty() {
                    Ok(vec![self.selected_commit_id()?])
                } else {
                    Ok(args.to_vec())
                }
            }
            "history.file" => {
                if args.is_empty() {
                    Ok(vec![
                        self.selected_file()?,
                        "0".to_string(),
                        "20".to_string(),
                    ])
                } else {
                    Ok(args.to_vec())
                }
            }
            "blame.file"
            | "diff.worktree"
            | "diff.index"
            | "file.discard"
            | "index.stage_paths"
            | "index.unstage_paths"
            | "conflict.resolve.ours"
            | "conflict.resolve.theirs"
            | "conflict.mark_resolved" => {
                if args.is_empty() {
                    self.selected_files()
                } else {
                    Ok(args.to_vec())
                }
            }
            "branch.checkout" | "branch.delete" => {
                if args.is_empty() {
                    Ok(vec![self.selected_branch_name()?])
                } else {
                    Ok(args.to_vec())
                }
            }
            "branch.rename" => {
                if args.len() == 1 {
                    Ok(vec![self.selected_branch_name()?, args[0].clone()])
                } else {
                    Ok(args.to_vec())
                }
            }
            "merge.execute" => {
                if args.is_empty() {
                    Ok(vec![self.selected_branch_name()?, "ff".to_string()])
                } else if args.len() == 1 && is_merge_mode(&args[0]) {
                    Ok(vec![self.selected_branch_name()?, args[0].clone()])
                } else {
                    Ok(args.to_vec())
                }
            }
            "compare.refs" => match args.len() {
                0 => Ok(vec![self.current_head_ref()?, self.selected_branch_name()?]),
                1 => Ok(vec![self.current_head_ref()?, args[0].clone()]),
                _ => Ok(args.to_vec()),
            },
            _ => Ok(args.to_vec()),
        }
    }

    fn sync_active_panel_after_op(&mut self, op: &str) {
        let view = match op {
            "status.refresh"
            | "index.stage_paths"
            | "index.unstage_paths"
            | "index.stage_hunk"
            | "index.stage_lines"
            | "index.unstage_hunk"
            | "index.unstage_lines"
            | "file.discard"
            | "file.discard_hunk"
            | "file.discard_lines"
            | "commit.create"
            | "commit.amend"
            | "stash.create"
            | "stash.list"
            | "stash.apply"
            | "stash.pop"
            | "stash.drop" => Some(PanelKind::Status.view_id()),
            "history.page"
            | "history.load_more"
            | "history.search"
            | "history.clear_filter"
            | "history.file"
            | "history.select_commit"
            | "history.details"
            | "blame.file"
            | "diff.commit"
            | "cherry_pick.commit"
            | "revert.commit" => Some(PanelKind::History.view_id()),
            "branch.checkout"
            | "branch.create"
            | "branch.rename"
            | "branch.delete"
            | "rebase.plan.create"
            | "rebase.plan.set_action"
            | "rebase.plan.move"
            | "rebase.plan.clear"
            | "rebase.execute"
            | "rebase.continue"
            | "rebase.skip"
            | "rebase.abort"
            | "merge.execute"
            | "merge.abort"
            | "reset.refs"
            | "conflict.focus"
            | "conflict.list"
            | "conflict.resolve.ours"
            | "conflict.resolve.theirs"
            | "conflict.mark_resolved"
            | "conflict.continue"
            | "conflict.abort" => Some(PanelKind::Branches.view_id()),
            "tag.create" | "tag.delete" | "tag.checkout" => Some(PanelKind::Tags.view_id()),
            "compare.refs" => Some(PanelKind::Compare.view_id()),
            "diagnostics.journal_summary" | "journal.open_entry" | "journal.copy_details" => {
                Some(PanelKind::Logs.view_id())
            }
            "diagnostics.repo_capabilities"
            | "diagnostics.lfs_status"
            | "diagnostics.lfs_fetch"
            | "diagnostics.lfs_pull" => Some(PanelKind::Diagnostics.view_id()),
            _ => None,
        };

        if let Some(view) = view {
            self.store.set_active_view(Some(view.to_string()));
        }
    }

    fn update_journal_summary_diff(&mut self) {
        self.store.update_diff(render_text_diff(
            "diagnostics:journal_summary",
            render_journal_summary(&self.store),
        ));
        self.last_replayable = Some(ReplayableRun::Run {
            target: "diagnostics.journal_summary".to_string(),
            args: Vec::new(),
        });
    }

    fn show_journal_summary(&mut self) {
        self.store
            .set_active_view(Some(PanelKind::Logs.view_id().to_string()));
        self.update_journal_summary_diff();
    }

    fn show_journal_entry(&mut self, entry_id: Option<u64>) -> Result<(), UserFacingError> {
        let details = self.render_journal_entry_details(entry_id)?;
        self.store
            .set_active_view(Some(PanelKind::Logs.view_id().to_string()));
        self.store
            .update_diff(render_text_diff("journal:entry_details", details));
        Ok(())
    }

    fn render_journal_entry_details(
        &self,
        entry_id: Option<u64>,
    ) -> Result<String, UserFacingError> {
        let entry = self
            .select_journal_entry(entry_id)
            .ok_or_else(|| invalid_input_error("journal entry not found"))?;
        let mut lines = vec![
            format!("Journal Entry #{}", entry.id),
            format!("Operation: {}", entry.op),
            format!("Status: {:?}", entry.status),
            format!(
                "Risk: {}",
                entry.risk.as_ref().map(danger_label).unwrap_or("unknown")
            ),
            format!(
                "Repository: {}",
                entry.repo_root.as_deref().unwrap_or("<unknown>")
            ),
            format!(
                "Params: {}",
                if entry.params.is_empty() {
                    "<none>".to_string()
                } else {
                    redact_params(&entry.params).join(" ")
                }
            ),
        ];
        if let Some(error) = entry.error.as_deref() {
            lines.push(format!("Error: {error}"));
            lines.push("Suggested next step: inspect repository status, then use recovery refs or reflog if refs moved.".to_string());
        }
        if let Some(pre_refs) = entry.pre_refs.as_ref() {
            lines.push(format_ref_snapshot("Pre refs", pre_refs));
        }
        if let Some(post_refs) = entry.post_refs.as_ref() {
            lines.push(format_ref_snapshot("Post refs", post_refs));
        }
        if !entry.backup_refs.is_empty() {
            lines.push("Backup refs:".to_string());
            for backup in &entry.backup_refs {
                lines.push(format!(
                    "- {} -> {} ({}, reason={})",
                    backup.name, backup.target_oid, backup.target_ref, backup.reason
                ));
            }
        }
        if let Some(explain) = explain_template_for_operation(&entry.op) {
            lines.push("Equivalent Git commands:".to_string());
            for command in explain.git_commands {
                lines.push(format!("- {command}"));
            }
            lines.push("Recovery notes:".to_string());
            for note in explain.recovery_notes {
                lines.push(format!("- {note}"));
            }
        }
        Ok(lines.join("\n"))
    }

    fn select_journal_entry(
        &self,
        entry_id: Option<u64>,
    ) -> Option<&state_store::OperationJournalEntry> {
        match entry_id {
            Some(id) => self
                .store
                .snapshot()
                .journal
                .entries
                .iter()
                .find(|entry| entry.id == id),
            None => self.store.snapshot().journal.entries.last(),
        }
    }

    fn resolve_restore_ref_args(&self, args: &[String]) -> Result<Vec<String>, UserFacingError> {
        if args.len() >= 2 {
            return Ok(vec![args[0].clone(), args[1].clone()]);
        }
        let entry_id = args.first().and_then(|value| value.parse::<u64>().ok());
        let entry = self
            .select_journal_entry(entry_id)
            .ok_or_else(|| invalid_input_error("journal entry not found"))?;
        let backup = entry
            .backup_refs
            .first()
            .ok_or_else(|| invalid_input_error("journal entry has no backup refs"))?;
        Ok(vec![backup.target_ref.clone(), backup.target_oid.clone()])
    }

    fn resolve_recover_operation_args(
        &self,
        args: &[String],
    ) -> Result<Vec<String>, UserFacingError> {
        let entry_id = args.first().and_then(|value| value.parse::<u64>().ok());
        let branch_name = args
            .get(1)
            .cloned()
            .unwrap_or_else(|| format!("branchforge/recovery/{}", entry_id.unwrap_or(0)));
        let entry = self
            .select_journal_entry(entry_id)
            .ok_or_else(|| invalid_input_error("journal entry not found"))?;
        let backup = entry
            .backup_refs
            .first()
            .ok_or_else(|| invalid_input_error("journal entry has no backup refs"))?;
        Ok(vec![branch_name, backup.name.clone()])
    }

    fn execute_in_open_repo(
        &mut self,
        op: &str,
        args: Vec<String>,
        replayable: bool,
    ) -> Result<JobExecutionResult, UserFacingError> {
        let repo_dir = self.require_repo_dir()?;
        let lock = lock_for_op(op, &args)?;
        self.execute_job(&repo_dir, op, lock, args, replayable)
    }

    fn execute_job(
        &mut self,
        cwd: &Path,
        op: &str,
        lock: JobLock,
        args: Vec<String>,
        replayable: bool,
    ) -> Result<JobExecutionResult, UserFacingError> {
        self.prepare_https_credentials(cwd, op, &args)?;
        let result = execute_job_op(
            cwd,
            &JobRequest {
                op: op.to_string(),
                lock,
                paths: args.clone(),
                job_id: None,
            },
            &mut self.store,
        )
        .map_err(|err| translate_job_error(&err))?;

        if let Some(repo) = self.store.repo() {
            self.repo_dir = Some(PathBuf::from(repo.root.clone()));
        }
        self.refresh_auth_state(cwd);

        if replayable && is_replayable_op(op) {
            self.last_replayable = Some(ReplayableRun::Run {
                target: op.to_string(),
                args,
            });
        }

        Ok(result)
    }

    fn refresh_selected_file_diff(&mut self) -> Result<(), UserFacingError> {
        let repo_dir = self.require_repo_dir()?;
        let selected = self.selected_files()?;
        let staged_only = selected.iter().all(|path| {
            self.store
                .snapshot()
                .status
                .staged
                .iter()
                .any(|item| item == path)
                && !self
                    .store
                    .snapshot()
                    .status
                    .unstaged
                    .iter()
                    .any(|item| item == path)
                && !self
                    .store
                    .snapshot()
                    .status
                    .untracked
                    .iter()
                    .any(|item| item == path)
        });
        let op = if staged_only {
            "diff.index"
        } else {
            "diff.worktree"
        };
        self.execute_job(&repo_dir, op, JobLock::Read, selected, true)?;
        Ok(())
    }

    fn run_replayable(&mut self, replayable: ReplayableRun) -> Result<(), UserFacingError> {
        match replayable {
            ReplayableRun::Run { target, args } => {
                if target.starts_with("plugin.") || target == "diagnostics.journal_summary" {
                    let _ = self.run_target(&target, &args, true)?;
                } else {
                    self.execute_in_open_repo(&target, args, true)?;
                }
            }
        }
        Ok(())
    }

    fn ensure_confirmation(
        &self,
        target: &str,
        args: &[String],
        confirmed: bool,
    ) -> Result<(), UserFacingError> {
        if confirmed {
            return Ok(());
        }
        if let Some(spec) = self.find_action(target)
            && spec.requires_confirmation()
        {
            return Err(confirmation_required_error(target, spec.effective_danger()));
        }

        if target == "reset.refs" {
            let mode = args.first().map(String::as_str).unwrap_or("mixed");
            let action_id = match mode {
                "soft" => "reset.soft",
                "mixed" => "reset.mixed",
                "hard" => "reset.hard",
                _ => "reset.mixed",
            };
            if let Some(spec) = self.find_action(action_id)
                && spec.requires_confirmation()
            {
                return Err(confirmation_required_error(
                    action_id,
                    spec.effective_danger(),
                ));
            }
        }

        Ok(())
    }

    fn selected_files(&self) -> Result<Vec<String>, UserFacingError> {
        if self.store.snapshot().selection.selected_paths.is_empty() {
            Err(invalid_input_error("select at least one file first"))
        } else {
            Ok(self.store.snapshot().selection.selected_paths.clone())
        }
    }

    fn selected_file(&self) -> Result<String, UserFacingError> {
        self.selected_files()?
            .into_iter()
            .next()
            .ok_or_else(|| invalid_input_error("select a file first"))
    }

    fn selected_commit_id(&self) -> Result<String, UserFacingError> {
        self.store
            .snapshot()
            .selection
            .selected_commit_oid
            .clone()
            .ok_or_else(|| invalid_input_error("select a commit first"))
    }

    fn selected_branch_name(&self) -> Result<String, UserFacingError> {
        self.store
            .snapshot()
            .selection
            .selected_branch
            .clone()
            .ok_or_else(|| invalid_input_error("select a branch first"))
    }

    fn selected_plugin_id(&self) -> Result<String, UserFacingError> {
        self.store
            .snapshot()
            .selection
            .selected_plugin_id
            .clone()
            .ok_or_else(|| invalid_input_error("select a plugin first"))
    }

    fn current_head_ref(&self) -> Result<String, UserFacingError> {
        self.store
            .snapshot()
            .repo
            .as_ref()
            .and_then(|repo| repo.head.clone())
            .ok_or_else(|| invalid_input_error("current HEAD is not available"))
    }

    fn require_repo_dir(&self) -> Result<PathBuf, UserFacingError> {
        self.repo_dir.clone().ok_or_else(|| {
            UserFacingError::with_category(
                "Repository required",
                "Open a repository first.",
                None,
                ErrorCategory::Repository,
            )
        })
    }

    fn find_action(&self, action_id: &str) -> Option<&ActionSpec> {
        self.actions
            .iter()
            .find(|action| action.spec.action_id == action_id)
            .map(|action| &action.spec)
    }

    fn action_owner_for(&self, action_id: &str) -> Option<&str> {
        self.actions
            .iter()
            .find(|action| action.spec.action_id == action_id)
            .map(|action| action.owner.as_str())
    }
}

fn build_builtin_catalog_actions() -> Vec<CatalogAction> {
    let mut actions = Vec::new();
    push_actions(
        &mut actions,
        "repo_manager",
        repo_manager_registration_payload().actions,
    );
    push_actions(
        &mut actions,
        "status",
        status_registration_payload().actions,
    );
    push_actions(
        &mut actions,
        "history",
        history_registration_payload().actions,
    );
    push_actions(
        &mut actions,
        "branches",
        branches_registration_payload().actions,
    );
    push_actions(&mut actions, "tags", tags_registration_payload().actions);
    push_actions(
        &mut actions,
        "compare",
        compare_registration_payload().actions,
    );
    push_actions(
        &mut actions,
        "diagnostics",
        diagnostics_registration_payload().actions,
    );
    push_actions(&mut actions, "diagnostics", host_plugin_action_specs());
    actions
}

fn map_graph_ref_label(raw: &str) -> Option<GraphRefLabel> {
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return None;
    }
    if let Some(name) = trimmed.strip_prefix("HEAD -> ") {
        return Some(GraphRefLabel {
            name: name.to_string(),
            kind: GraphRefKind::Head,
        });
    }
    if let Some(name) = trimmed.strip_prefix("tag: ") {
        return Some(GraphRefLabel {
            name: name.to_string(),
            kind: GraphRefKind::Tag,
        });
    }
    if trimmed == "HEAD" {
        return Some(GraphRefLabel {
            name: "HEAD".to_string(),
            kind: GraphRefKind::Head,
        });
    }
    let kind = if trimmed.contains('/') {
        GraphRefKind::RemoteBranch
    } else {
        GraphRefKind::LocalBranch
    };
    Some(GraphRefLabel {
        name: trimmed.to_string(),
        kind,
    })
}

fn base_operation_preview(
    action_id: &str,
    danger: DangerLevel,
    explain: Option<&ExplainTemplate>,
    args: &[String],
) -> OperationPreview {
    let summary = explain
        .map(|template| template.plain_summary.clone())
        .unwrap_or_else(|| format!("Run {action_id}."));
    let git_commands = explain
        .map(|template| template.git_commands.clone())
        .unwrap_or_else(|| vec![format!("branchforge run --confirm {action_id}")]);
    OperationPreview {
        operation: action_id.to_string(),
        danger,
        summary,
        affected_refs: Vec::new(),
        affected_files: args
            .iter()
            .filter(|arg| looks_like_path_arg(arg))
            .map(|path| FileImpact {
                path: path.clone(),
                impact: "may be affected".to_string(),
                detail: None,
            })
            .collect(),
        commits_rewritten: Vec::new(),
        worktree_impact: impact(ImpactLevel::Read, "No direct worktree impact detected yet."),
        index_impact: impact(ImpactLevel::Read, "No direct index impact detected yet."),
        remote_impact: None,
        warnings: explain
            .map(|template| {
                template
                    .risks
                    .iter()
                    .map(|risk| warning(PreviewWarningLevel::Warning, risk.clone()))
                    .collect()
            })
            .unwrap_or_default(),
        recommended_action: explain.and_then(|template| template.recovery_notes.first().cloned()),
        git_commands,
    }
}

fn looks_like_path_arg(arg: &str) -> bool {
    arg.contains('/') || arg.contains('.') || arg.starts_with("./")
}

fn workspace_store_path_for(cwd: &Path) -> PathBuf {
    cwd.join("target/tmp/branchforge-workspaces.json")
}

fn current_millis() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as u64
}

fn unique_workspace_id(state: &WorkspaceState, name: &str) -> String {
    let base = slug_identifier("workspace", name);
    if !state
        .workspaces
        .iter()
        .any(|workspace| workspace.id == base)
    {
        return base;
    }
    let mut idx = 2usize;
    loop {
        let candidate = format!("{base}-{idx}");
        if !state
            .workspaces
            .iter()
            .any(|workspace| workspace.id == candidate)
        {
            return candidate;
        }
        idx += 1;
    }
}

fn stack_id_for(name: &str) -> String {
    slug_identifier("stack", name)
}

fn slug_identifier(prefix: &str, value: &str) -> String {
    let slug = value
        .chars()
        .map(|ch| {
            if ch.is_ascii_alphanumeric() {
                ch.to_ascii_lowercase()
            } else {
                '-'
            }
        })
        .collect::<String>()
        .split('-')
        .filter(|part| !part.is_empty())
        .collect::<Vec<_>>()
        .join("-");
    if slug.is_empty() {
        prefix.to_string()
    } else {
        format!("{prefix}-{slug}")
    }
}

fn ensure_active_workspace(state: &mut WorkspaceState) -> usize {
    if let Some(idx) = active_workspace_index(state) {
        return idx;
    }
    if state.workspaces.is_empty() {
        state.workspaces.push(Workspace {
            id: "workspace-default".to_string(),
            name: "Default Workspace".to_string(),
            repos: Vec::new(),
            groups: Vec::new(),
            last_opened_ms: current_millis(),
        });
    }
    state.active_workspace_id = Some(state.workspaces[0].id.clone());
    0
}

fn active_workspace_index(state: &WorkspaceState) -> Option<usize> {
    state
        .active_workspace_id
        .as_ref()
        .and_then(|id| find_workspace_index(state, id))
        .or_else(|| (!state.workspaces.is_empty()).then_some(0))
}

fn find_workspace_index(state: &WorkspaceState, value: &str) -> Option<usize> {
    state
        .workspaces
        .iter()
        .position(|workspace| workspace.id == value || workspace.name == value)
}

fn find_workspace_repo(state: &WorkspaceState, value: &str) -> Option<(usize, WorkspaceRepo)> {
    state
        .workspaces
        .iter()
        .enumerate()
        .find_map(|(workspace_idx, workspace)| {
            workspace
                .repos
                .iter()
                .find(|repo| {
                    repo.repo_id == value || repo.path == value || repo.display_name == value
                })
                .cloned()
                .map(|repo| (workspace_idx, repo))
        })
}

fn workspace_repo_record(
    root: &Path,
    group_id: Option<String>,
) -> Result<WorkspaceRepo, UserFacingError> {
    let (status_summary, branch_summary) =
        summarize_workspace_repo(root).unwrap_or_else(|message| {
            (
                RepoStatusSummary {
                    last_error: Some(message),
                    ..RepoStatusSummary::default()
                },
                RepoBranchSummary::default(),
            )
        });
    let display_name = root
        .file_name()
        .and_then(|value| value.to_str())
        .unwrap_or("repository")
        .to_string();
    let path = root.to_string_lossy().to_string();
    Ok(WorkspaceRepo {
        repo_id: path.clone(),
        path,
        display_name,
        group_id,
        status_summary,
        branch_summary,
    })
}

fn summarize_workspace_repo(path: &Path) -> Result<(RepoStatusSummary, RepoBranchSummary), String> {
    let repo = git_service::repo_open(path).map_err(|err| format!("{err:?}"))?;
    let status = git_service::status_refresh(path).map_err(|err| format!("{err:?}"))?;
    let upstream = git_service::upstream_status(path).unwrap_or_default();
    Ok((
        RepoStatusSummary {
            dirty: !status.staged.is_empty()
                || !status.unstaged.is_empty()
                || !status.untracked.is_empty()
                || repo.conflict_state.is_some(),
            staged: status.staged.len(),
            unstaged: status.unstaged.len(),
            untracked: status.untracked.len(),
            conflicts: repo.conflict_state.is_some(),
            detached: repo.detached,
            last_error: None,
        },
        RepoBranchSummary {
            current_branch: upstream.current_branch.or(repo.head),
            upstream: upstream.upstream,
            ahead: upstream.ahead,
            behind: upstream.behind,
        },
    ))
}

fn map_provider_repository(provider: git_service::ProviderRepository) -> ProviderRepository {
    ProviderRepository {
        provider: match provider.provider {
            git_service::ProviderKind::GitHub => state_store::ProviderKind::GitHub,
            git_service::ProviderKind::GitLab => state_store::ProviderKind::GitLab,
            git_service::ProviderKind::Unknown => state_store::ProviderKind::Unknown,
        },
        host: provider.host,
        owner: provider.owner,
        repo: provider.repo,
        web_url: provider.web_url,
    }
}

fn map_provider_kind_for_git(provider: &state_store::ProviderKind) -> git_service::ProviderKind {
    match provider {
        state_store::ProviderKind::GitHub => git_service::ProviderKind::GitHub,
        state_store::ProviderKind::GitLab => git_service::ProviderKind::GitLab,
        state_store::ProviderKind::Unknown => git_service::ProviderKind::Unknown,
    }
}

fn parse_provider_kind(raw: &str) -> Option<state_store::ProviderKind> {
    match raw.trim().to_ascii_lowercase().as_str() {
        "github" | "github.com" => Some(state_store::ProviderKind::GitHub),
        "gitlab" | "gitlab.com" => Some(state_store::ProviderKind::GitLab),
        "unknown" => Some(state_store::ProviderKind::Unknown),
        _ => None,
    }
}

fn https_host_from_url(url: &str) -> Option<String> {
    let rest = url.strip_prefix("https://")?;
    let authority = rest.split('/').next().unwrap_or_default();
    let host = authority
        .rsplit('@')
        .next()
        .unwrap_or(authority)
        .split(':')
        .next()
        .unwrap_or_default()
        .trim()
        .to_ascii_lowercase();
    (!host.is_empty()).then_some(host)
}

fn render_auth_summary(auth: &AuthStatus) -> String {
    let mut lines = vec![
        "Authentication Status".to_string(),
        format!("ssh-agent: {}", auth.ssh_agent_available),
        format!("git credential helper: {}", auth.https_helper_configured),
    ];
    if auth.accounts.is_empty() {
        lines.push("stored accounts: none".to_string());
    } else {
        lines.push("stored accounts:".to_string());
        for account in &auth.accounts {
            lines.push(format!(
                "- {} {} provider={:?} token_present={}",
                account.host,
                account.username.as_deref().unwrap_or("<unknown>"),
                account.provider,
                account.token_present
            ));
        }
    }
    if let Some(error) = auth.last_error.as_deref() {
        lines.push(format!("last error: {error}"));
    }
    lines.join("\n")
}

fn create_pull_request_url(
    provider: &ProviderRepository,
    base: &str,
    head: &str,
    title: Option<&str>,
) -> String {
    match provider.provider {
        state_store::ProviderKind::GitHub => {
            let mut url = format!("{}/compare/{base}...{head}?expand=1", provider.web_url);
            if let Some(title) = title.filter(|title| !title.trim().is_empty()) {
                url.push_str("&title=");
                url.push_str(&url_query_escape(title));
            }
            url
        }
        state_store::ProviderKind::GitLab => {
            let mut url = format!(
                "{}/-/merge_requests/new?merge_request[source_branch]={}&merge_request[target_branch]={}",
                provider.web_url,
                url_query_escape(head),
                url_query_escape(base)
            );
            if let Some(title) = title.filter(|title| !title.trim().is_empty()) {
                url.push_str("&merge_request[title]=");
                url.push_str(&url_query_escape(title));
            }
            url
        }
        state_store::ProviderKind::Unknown => {
            format!("{}/compare/{base}...{head}", provider.web_url)
        }
    }
}

fn url_query_escape(value: &str) -> String {
    let mut escaped = String::new();
    for byte in value.bytes() {
        if byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'.' | b'~') {
            escaped.push(byte as char);
        } else {
            escaped.push_str(&format!("%{byte:02X}"));
        }
    }
    escaped
}

fn pull_request_summary_for_url(
    provider: &ProviderRepository,
    base: &str,
    head: &str,
    url: String,
) -> PullRequestSummary {
    PullRequestSummary {
        provider: provider.provider.clone(),
        repo: format!("{}/{}", provider.owner, provider.repo),
        number: 0,
        title: format!("Create PR for {head}"),
        author: "local".to_string(),
        source_branch: head.to_string(),
        target_branch: base.to_string(),
        state: PullRequestState::Draft,
        checks: vec![state_store::CheckSummary {
            name: "provider-api".to_string(),
            status: CheckStatus::Pending,
            detail: Some(
                "Use pr.list with a stored provider token to load live checks.".to_string(),
            ),
        }],
        review_state: Some(ReviewState::ReviewRequired),
        web_url: Some(url),
    }
}

fn build_stack_entry(
    repo_dir: &Path,
    base_branch: &str,
    branch: &str,
) -> Result<BranchStackEntry, UserFacingError> {
    let head_oid = git_service::resolve_ref_oid(repo_dir, branch).map_err(|err| {
        UserFacingError::with_category(
            "Stack branch failed",
            "Could not resolve stack branch.",
            Some(format!("{err:?}")),
            ErrorCategory::Git,
        )
    })?;
    let compare = git_service::compare_refs(repo_dir, base_branch, branch, 50).map_err(|err| {
        UserFacingError::with_category(
            "Stack comparison failed",
            "Could not compare stack branch with its base.",
            Some(format!("{err:?}")),
            ErrorCategory::Git,
        )
    })?;
    Ok(BranchStackEntry {
        branch: branch.to_string(),
        base_branch: base_branch.to_string(),
        head_oid,
        upstream_pr: None,
        status: if compare.behind > 0 {
            StackEntryStatus::NeedsRestack
        } else {
            StackEntryStatus::Clean
        },
        ahead: compare.ahead,
        behind: compare.behind,
    })
}

fn upsert_stack(state: &mut BranchStackState, stack: BranchStack) {
    if let Some(existing) = state.stacks.iter_mut().find(|item| item.id == stack.id) {
        *existing = stack;
    } else {
        state.stacks.push(stack);
    }
}

fn impact(level: ImpactLevel, summary: impl Into<String>) -> ImpactSummary {
    ImpactSummary {
        level,
        summary: summary.into(),
    }
}

fn warning(level: PreviewWarningLevel, message: impl Into<String>) -> PreviewWarning {
    PreviewWarning {
        level,
        message: message.into(),
    }
}

pub fn explain_template_for_action(action_id: &str) -> Option<ExplainTemplate> {
    let template = match action_id {
        "reset.soft" => explain(
            action_id,
            "This moves the current branch to the target commit while preserving staged and working tree changes.",
            &["git reset --soft <target>"],
            &[
                "Branch tip changes; commit hashes after the target may become unreachable from this branch.",
            ],
            &["Restore the previous branch tip from the operation journal backup ref or reflog."],
        ),
        "reset.mixed" => explain(
            action_id,
            "This moves the current branch to the target commit and makes the index match that commit.",
            &["git reset --mixed <target>"],
            &["Staged changes are unstaged or replaced; branch tip changes."],
            &["Restore the previous branch tip from the operation journal backup ref or reflog."],
        ),
        "reset.hard" => explain(
            action_id,
            "This moves the current branch to the target commit and makes the index and working tree match it.",
            &["git reset --hard <target>"],
            &["Uncommitted worktree changes and staged changes can be overwritten."],
            &[
                "Use the operation journal backup ref or reflog for committed work; stash or patch snapshots are needed for uncommitted work.",
            ],
        ),
        "commit.create" => explain(
            action_id,
            "This creates a new commit from the staged index.",
            &["git commit -m <message>"],
            &["The commit records exactly what is staged; unstaged changes are left out."],
            &["Use revert for published commits or reset/reflog for local-only history recovery."],
        ),
        "commit.amend" => explain(
            action_id,
            "This replaces the latest commit with a new commit using the staged index and message.",
            &["git commit --amend"],
            &["The previous commit hash is replaced; published history may need coordination."],
            &["Restore the previous HEAD from the operation journal or reflog."],
        ),
        "branch.checkout" => explain(
            action_id,
            "This switches the working tree and index to another branch.",
            &["git checkout <branch>"],
            &["Dirty files can block checkout or be affected by branch content changes."],
            &["Return to the previous branch or use reflog for recent branch movements."],
        ),
        "branch.create" => explain(
            action_id,
            "This creates a new branch ref.",
            &["git branch <name> [base]"],
            &["Creating from the wrong base can start work from an unintended commit."],
            &["Delete or rename the branch, or recreate it from the intended base."],
        ),
        "branch.rename" => explain(
            action_id,
            "This renames a local branch ref.",
            &["git branch -m <old> <new>"],
            &["Automation or remotes may still refer to the old branch name."],
            &["Rename back or recreate the old branch from the journal snapshot."],
        ),
        "branch.delete" => explain(
            action_id,
            "This removes a local branch ref.",
            &["git branch -d <branch>", "git branch -D <branch>"],
            &["Unmerged branch commits can become hard to find after deletion."],
            &["Recreate the branch from the BranchForge backup ref or from a reflog entry."],
        ),
        "tag.delete" => explain(
            action_id,
            "This removes a local tag ref.",
            &["git tag -d <tag>"],
            &["Deleted tags no longer protect or name the tagged object."],
            &["Restore the tag ref from the operation journal backup ref if available."],
        ),
        "tag.checkout" => explain(
            action_id,
            "This checks out a tag and usually leaves the repository in detached HEAD state.",
            &["git checkout <tag>"],
            &[
                "New commits made from detached HEAD can become hard to find unless a branch is created.",
            ],
            &["Create a branch from the detached commit or use reflog to recover work."],
        ),
        "remote.fetch" | "remote.fetch_all" => explain(
            action_id,
            "This contacts configured remotes and updates remote-tracking refs.",
            &["git fetch <remote>", "git fetch --all"],
            &["Network auth can fail; remote-tracking refs can move after fetch."],
            &["Inspect remote branches and ahead/behind counts before merging or rebasing."],
        ),
        "remote.pull" => explain(
            action_id,
            "This fetches and fast-forwards the current branch from its upstream.",
            &["git pull --ff-only"],
            &["The working tree and current branch can change when a fast-forward is available."],
            &["Use the operation journal and reflog if the branch moved unexpectedly."],
        ),
        "remote.push" => explain(
            action_id,
            "This publishes the current branch to its configured upstream.",
            &["git push"],
            &["Remote rejects can happen when the branch is behind or credentials are missing."],
            &["Fetch first and inspect ahead/behind counts before retrying."],
        ),
        "remote.push_set_upstream" => explain(
            action_id,
            "This pushes a branch and records its upstream remote tracking configuration.",
            &["git push -u <remote> <branch>"],
            &["The chosen remote becomes the branch's default push/pull target."],
            &["Use git branch --unset-upstream or set a different upstream if needed."],
        ),
        "remote.remove" => explain(
            action_id,
            "This removes a local remote configuration entry.",
            &["git remote remove <name>"],
            &["Fetch/push shortcuts and remote branch views for that remote stop working locally."],
            &["Add the remote again with the same URL if it was removed by mistake."],
        ),
        "remote.push_force_with_lease" => explain(
            action_id,
            "This force pushes only if the remote branch still matches the last known remote-tracking ref.",
            &["git push --force-with-lease <remote> <branch>"],
            &["Remote history can be rewritten for collaborators if the lease matches."],
            &[
                "Fetch first, inspect ahead/behind counts, and recover local refs from the journal or reflog.",
            ],
        ),
        "workspace.fetch_all" => explain(
            action_id,
            "This fetches every repository in the active workspace.",
            &["git fetch --all"],
            &["Each repository can prompt for credentials or fail independently."],
            &["Review per-repository workspace results before pulling or rebasing."],
        ),
        "pr.checkout" => explain(
            action_id,
            "This fetches a provider pull request ref into a local branch and checks it out.",
            &[
                "git fetch <remote> pull/<number>/head:<branch>",
                "git checkout <branch>",
            ],
            &["The working tree and current branch change; provider refs require network access."],
            &["Return to the previous branch or use reflog if checkout was not intended."],
        ),
        "stack.restack" | "stack.restack_branch" => explain(
            action_id,
            "This rebases stack branches onto their configured parent branches.",
            &["git checkout <branch>", "git rebase <base>"],
            &["Stack branch history is rewritten and conflicts can interrupt the sequence."],
            &[
                "Each branch restack is journaled with a backup ref so the previous branch tip can be recovered.",
            ],
        ),
        "file.discard" | "file.discard_hunk" | "file.discard_lines" => explain(
            action_id,
            "This discards selected working tree changes.",
            &["git checkout -- <path>", "git apply -R <patch>"],
            &["Uncommitted work can be lost if no patch snapshot exists."],
            &[
                "Recover from a patch snapshot when available; otherwise inspect editor history or filesystem backups.",
            ],
        ),
        "stash.create" => explain(
            action_id,
            "This saves local work as a stash entry and cleans the working tree.",
            &["git stash push --include-untracked"],
            &["Untracked and staged context moves into the stash entry."],
            &["Apply or pop the stash later; inspect stash reflog if an entry is lost."],
        ),
        "stash.apply" => explain(
            action_id,
            "This applies a stash entry to the working tree without dropping it.",
            &["git stash apply <stash>"],
            &["Overlapping changes can conflict."],
            &[
                "Abort by resolving or resetting affected files, then inspect the journal if needed.",
            ],
        ),
        "stash.pop" => explain(
            action_id,
            "This applies a stash to the working tree and drops it if the apply succeeds.",
            &["git stash pop <stash>"],
            &[
                "Overlapping changes can conflict; the stash entry is removed after a successful pop.",
            ],
            &[
                "If a pop succeeds but the result is unwanted, recover with the operation journal or reflog where possible.",
            ],
        ),
        "stash.drop" => explain(
            action_id,
            "This removes a stash entry.",
            &["git stash drop <stash>"],
            &["Dropped stash entries are not shown in the stash list."],
            &["Use reflog entries for stash refs when available."],
        ),
        "worktree.create" => explain(
            action_id,
            "This creates an additional working tree for a branch.",
            &["git worktree add <path> <branch>"],
            &["A new checkout path is created on disk."],
            &["Remove the worktree when finished; the main repository refs remain recoverable."],
        ),
        "worktree.remove" => explain(
            action_id,
            "This removes a linked working tree path.",
            &["git worktree remove <path>"],
            &["Uncommitted files in that worktree can be lost."],
            &["Inspect or stash the linked worktree before removal."],
        ),
        "submodule.init_update" => explain(
            action_id,
            "This initializes or updates submodule checkouts.",
            &["git submodule update --init --recursive"],
            &["Submodule working trees can change on disk and may require network access."],
            &["Use submodule status and reflog inside the submodule for recovery."],
        ),
        "merge.execute" => explain(
            action_id,
            "This merges the selected source ref into the current branch.",
            &["git merge <source>"],
            &["Conflicts may stop the merge; a merge commit can change branch history."],
            &["Abort an active merge or restore the pre-merge branch tip from the journal."],
        ),
        "merge.abort" => explain(
            action_id,
            "This aborts the active merge session.",
            &["git merge --abort"],
            &["Conflict resolution edits made during the merge can be overwritten."],
            &[
                "Use the journal or reflog if the abort does not return the repository to the expected state.",
            ],
        ),
        "rebase.plan.create" => explain(
            action_id,
            "This prepares an interactive rebase plan for review.",
            &["git log <base>..HEAD"],
            &["Choosing the wrong base can select an unintended commit range."],
            &["Clear the plan or regenerate it from the intended base."],
        ),
        "rebase.execute" | "rebase.interactive" => explain(
            action_id,
            "This rewrites selected commits on top of the chosen base. Commit hashes will change.",
            &["git rebase -i <base>"],
            &[
                "Published history may require force push coordination; conflicts can pause the rebase.",
            ],
            &[
                "Abort while active, or create a recovery branch from the backup ref after completion.",
            ],
        ),
        "rebase.continue" => explain(
            action_id,
            "This continues an active rebase after conflicts or edit steps.",
            &["git rebase --continue"],
            &["Continuing records the current index as the next rewritten commit."],
            &["Abort while active or restore from the pre-rebase backup branch afterward."],
        ),
        "rebase.skip" => explain(
            action_id,
            "This skips the current commit during an active rebase.",
            &["git rebase --skip"],
            &["Skipping drops the current commit from the rebased history."],
            &[
                "Restore from the pre-rebase backup branch or reflog if the skipped commit is needed.",
            ],
        ),
        "rebase.abort" => explain(
            action_id,
            "This aborts the active rebase session.",
            &["git rebase --abort"],
            &["Conflict resolution edits made during the rebase can be overwritten."],
            &[
                "Use the journal backup ref or reflog if the abort cannot restore the expected branch tip.",
            ],
        ),
        "conflict.resolve.ours" => explain(
            action_id,
            "This resolves selected conflicted files by taking our side.",
            &["git checkout --ours -- <path>", "git add <path>"],
            &["The other side of the conflict is discarded for those files."],
            &["Use the operation journal and file history to inspect or redo resolution choices."],
        ),
        "conflict.resolve.theirs" => explain(
            action_id,
            "This resolves selected conflicted files by taking their side.",
            &["git checkout --theirs -- <path>", "git add <path>"],
            &["Your side of the conflict is discarded for those files."],
            &["Use the operation journal and file history to inspect or redo resolution choices."],
        ),
        "conflict.mark_resolved" => explain(
            action_id,
            "This stages selected conflicted files as resolved.",
            &["git add <path>"],
            &["Unresolved markers can be committed if the file was not inspected."],
            &["Use diff and conflict marker checks before continuing."],
        ),
        "conflict.continue" => explain(
            action_id,
            "This continues the active merge, rebase, or cherry-pick after conflicts are resolved.",
            &[
                "git merge --continue",
                "git rebase --continue",
                "git cherry-pick --continue",
            ],
            &["Continuing with unresolved files can fail or record an unintended resolution."],
            &["Use abort while the session is active, or journal backup refs afterward."],
        ),
        "conflict.abort" => explain(
            action_id,
            "This aborts the active conflict-producing operation.",
            &[
                "git merge --abort",
                "git rebase --abort",
                "git cherry-pick --abort",
            ],
            &["Conflict resolution edits made during the session can be overwritten."],
            &["Use operation journal backup refs for committed history recovery."],
        ),
        "cherry_pick.commit" => explain(
            action_id,
            "This applies the selected commit onto the current branch.",
            &["git cherry-pick <commit>"],
            &["Conflicts may pause the cherry-pick."],
            &["Abort an active cherry-pick or reset to the pre-operation backup ref."],
        ),
        "revert.commit" => explain(
            action_id,
            "This creates a new commit that reverses the selected commit.",
            &["git revert --no-edit <commit>"],
            &["Conflicts may pause the revert; merge commits require extra parent selection."],
            &["Abort an active revert/cherry-pick session or revert the revert commit."],
        ),
        "journal.clear_old_entries" => explain(
            action_id,
            "This removes older operation journal entries while keeping the newest entries.",
            &["branchforge run journal.clear_old_entries <keep_latest>"],
            &["Cleared entries no longer appear as recovery starting points in the UI."],
            &["Export the journal before clearing if audit history is needed."],
        ),
        "journal.restore_ref" | "recovery.restore_ref" => explain(
            action_id,
            "This moves a Git ref back to a saved object id.",
            &["git update-ref <ref> <oid>"],
            &["Moving a ref changes what branch or tag name points to."],
            &["The recovery operation is journaled with pre/post ref snapshots."],
        ),
        "journal.recover_operation" | "recovery.create_branch_from_backup" => explain(
            action_id,
            "This creates a recovery branch from a BranchForge backup ref.",
            &["git branch <name> <backup-ref>"],
            &["The new branch name must not already exist."],
            &["Use the branch to inspect or cherry-pick recovered commits."],
        ),
        "recovery.create_branch_from_reflog" => explain(
            action_id,
            "This creates a branch at a selected reflog entry.",
            &["git branch <name> <reflog-oid>"],
            &["The reflog entry may point to an older state that lacks later work."],
            &["Use the branch to inspect recovered history before merging or resetting."],
        ),
        "plugin.remove" => explain(
            action_id,
            "This removes an installed plugin package from the configured plugins root.",
            &["branchforge plugin remove <plugin-id>"],
            &["Removing a plugin disables its actions and local package files."],
            &["Reinstall the plugin from the original package or registry if needed."],
        ),
        _ => return None,
    };
    Some(template)
}

fn explain(
    action_id: &str,
    plain_summary: &str,
    git_commands: &[&str],
    risks: &[&str],
    recovery_notes: &[&str],
) -> ExplainTemplate {
    ExplainTemplate {
        action_id: action_id.to_string(),
        plain_summary: plain_summary.to_string(),
        git_commands: git_commands.iter().map(|value| value.to_string()).collect(),
        risks: risks.iter().map(|value| value.to_string()).collect(),
        recovery_notes: recovery_notes
            .iter()
            .map(|value| value.to_string())
            .collect(),
    }
}

fn push_actions(actions: &mut Vec<CatalogAction>, owner: &str, specs: Vec<ActionSpec>) {
    actions.extend(specs.into_iter().map(|spec| CatalogAction {
        owner: owner.to_string(),
        spec,
    }));
}

fn host_action_spec(
    action_id: &str,
    title: &str,
    when: Option<&str>,
    danger: Option<DangerLevel>,
    effects: ActionEffects,
    confirm_policy: ConfirmPolicy,
) -> ActionSpec {
    ActionSpec {
        action_id: action_id.to_string(),
        title: title.to_string(),
        when: when.map(str::to_string),
        params_schema: None,
        danger,
        effects,
        confirm_policy,
    }
}

fn host_plugin_action_specs() -> Vec<ActionSpec> {
    vec![
        host_action_spec(
            "ops.check_deps",
            "Check Dependency Guards",
            Some("always"),
            None,
            ActionEffects::read_only(),
            ConfirmPolicy::Never,
        ),
        host_action_spec(
            "ops.dev_check",
            "Run Dev Check",
            Some("always"),
            Some(DangerLevel::Low),
            ActionEffects {
                writes_worktree: true,
                danger_level: DangerLevel::Low,
                ..ActionEffects::default()
            },
            ConfirmPolicy::Never,
        ),
        host_action_spec(
            "release.notes",
            "Generate Release Notes",
            Some("always"),
            Some(DangerLevel::Low),
            ActionEffects {
                writes_worktree: true,
                danger_level: DangerLevel::Low,
                ..ActionEffects::default()
            },
            ConfirmPolicy::Never,
        ),
        host_action_spec(
            "release.sign",
            "Sign Artifacts",
            Some("always"),
            Some(DangerLevel::Low),
            ActionEffects {
                writes_worktree: true,
                danger_level: DangerLevel::Low,
                ..ActionEffects::default()
            },
            ConfirmPolicy::Never,
        ),
        host_action_spec(
            "release.package_local",
            "Create Local Package",
            Some("always"),
            Some(DangerLevel::Low),
            ActionEffects {
                writes_worktree: true,
                danger_level: DangerLevel::Low,
                ..ActionEffects::default()
            },
            ConfirmPolicy::Never,
        ),
        host_action_spec(
            "release.package",
            "Create Release Package",
            Some("always"),
            Some(DangerLevel::Low),
            ActionEffects {
                writes_worktree: true,
                danger_level: DangerLevel::Low,
                ..ActionEffects::default()
            },
            ConfirmPolicy::Never,
        ),
        host_action_spec(
            "release.verify",
            "Verify Release Package",
            Some("always"),
            Some(DangerLevel::Low),
            ActionEffects {
                writes_worktree: true,
                danger_level: DangerLevel::Low,
                ..ActionEffects::default()
            },
            ConfirmPolicy::Never,
        ),
        host_action_spec(
            "verify.sprint22",
            "Verify Sprint 22",
            Some("always"),
            Some(DangerLevel::Low),
            ActionEffects {
                writes_worktree: true,
                danger_level: DangerLevel::Low,
                ..ActionEffects::default()
            },
            ConfirmPolicy::Never,
        ),
        host_action_spec(
            "verify.sprint23",
            "Verify Sprint 23",
            Some("always"),
            Some(DangerLevel::Low),
            ActionEffects {
                writes_worktree: true,
                danger_level: DangerLevel::Low,
                ..ActionEffects::default()
            },
            ConfirmPolicy::Never,
        ),
        host_action_spec(
            "verify.sprint24",
            "Verify Sprint 24",
            Some("always"),
            Some(DangerLevel::Low),
            ActionEffects {
                writes_worktree: true,
                danger_level: DangerLevel::Low,
                ..ActionEffects::default()
            },
            ConfirmPolicy::Never,
        ),
        host_action_spec(
            "plugin.list",
            "List Plugins",
            Some("always"),
            None,
            ActionEffects::read_only(),
            ConfirmPolicy::Never,
        ),
        host_action_spec(
            "journal.open_entry",
            "Open Journal Entry",
            Some("always"),
            None,
            ActionEffects::read_only(),
            ConfirmPolicy::Never,
        ),
        host_action_spec(
            "journal.copy_details",
            "Copy Journal Details",
            Some("always"),
            None,
            ActionEffects::read_only(),
            ConfirmPolicy::Never,
        ),
        host_action_spec(
            "journal.export",
            "Export Journal",
            Some("always"),
            Some(DangerLevel::Low),
            ActionEffects {
                writes_worktree: true,
                danger_level: DangerLevel::Low,
                ..ActionEffects::default()
            },
            ConfirmPolicy::Never,
        ),
        host_action_spec(
            "journal.clear_old_entries",
            "Clear Old Journal Entries",
            Some("always"),
            Some(DangerLevel::Medium),
            ActionEffects {
                writes_worktree: true,
                danger_level: DangerLevel::Medium,
                ..ActionEffects::default()
            },
            ConfirmPolicy::OnDanger,
        ),
        host_action_spec(
            "auth.status",
            "Show Auth Status",
            Some("always"),
            None,
            ActionEffects::read_only(),
            ConfirmPolicy::Never,
        ),
        host_action_spec(
            "auth.login",
            "Store HTTPS Token",
            Some("always"),
            Some(DangerLevel::Low),
            ActionEffects {
                danger_level: DangerLevel::Low,
                ..ActionEffects::default()
            },
            ConfirmPolicy::Never,
        ),
        host_action_spec(
            "auth.logout",
            "Remove HTTPS Token",
            Some("always"),
            Some(DangerLevel::Medium),
            ActionEffects {
                danger_level: DangerLevel::Medium,
                ..ActionEffects::default()
            },
            ConfirmPolicy::OnDanger,
        ),
        host_action_spec(
            "auth.seed_git",
            "Approve Git Credential",
            Some("repo.is_open"),
            Some(DangerLevel::Low),
            ActionEffects {
                danger_level: DangerLevel::Low,
                ..ActionEffects::default()
            },
            ConfirmPolicy::Never,
        ),
        host_action_spec(
            "remote.refresh",
            "Refresh Remotes",
            Some("repo.is_open"),
            None,
            ActionEffects::read_only(),
            ConfirmPolicy::Never,
        ),
        host_action_spec(
            "remote.add",
            "Add Remote",
            Some("repo.is_open"),
            Some(DangerLevel::Medium),
            ActionEffects {
                writes_refs: true,
                danger_level: DangerLevel::Medium,
                ..ActionEffects::default()
            },
            ConfirmPolicy::OnDanger,
        ),
        host_action_spec(
            "remote.remove",
            "Remove Remote",
            Some("repo.is_open"),
            Some(DangerLevel::High),
            ActionEffects::mutating_refs(),
            ConfirmPolicy::Always,
        ),
        host_action_spec(
            "remote.rename",
            "Rename Remote",
            Some("repo.is_open"),
            Some(DangerLevel::Medium),
            ActionEffects {
                writes_refs: true,
                danger_level: DangerLevel::Medium,
                ..ActionEffects::default()
            },
            ConfirmPolicy::OnDanger,
        ),
        host_action_spec(
            "remote.fetch",
            "Fetch Remote",
            Some("repo.is_open"),
            Some(DangerLevel::Low),
            ActionEffects {
                network: true,
                danger_level: DangerLevel::Low,
                ..ActionEffects::default()
            },
            ConfirmPolicy::Never,
        ),
        host_action_spec(
            "remote.fetch_all",
            "Fetch All Remotes",
            Some("repo.is_open"),
            Some(DangerLevel::Low),
            ActionEffects {
                network: true,
                danger_level: DangerLevel::Low,
                ..ActionEffects::default()
            },
            ConfirmPolicy::Never,
        ),
        host_action_spec(
            "remote.pull",
            "Pull Current Branch",
            Some("repo.is_open"),
            Some(DangerLevel::Medium),
            ActionEffects {
                writes_refs: true,
                writes_worktree: true,
                network: true,
                danger_level: DangerLevel::Medium,
                ..ActionEffects::default()
            },
            ConfirmPolicy::OnDanger,
        ),
        host_action_spec(
            "remote.push",
            "Push Current Branch",
            Some("repo.is_open"),
            Some(DangerLevel::Medium),
            ActionEffects {
                network: true,
                danger_level: DangerLevel::Medium,
                ..ActionEffects::default()
            },
            ConfirmPolicy::OnDanger,
        ),
        host_action_spec(
            "remote.push_set_upstream",
            "Push And Set Upstream",
            Some("repo.is_open"),
            Some(DangerLevel::Medium),
            ActionEffects {
                writes_refs: true,
                network: true,
                danger_level: DangerLevel::Medium,
                ..ActionEffects::default()
            },
            ConfirmPolicy::OnDanger,
        ),
        host_action_spec(
            "remote.push_force_with_lease",
            "Force Push With Lease",
            Some("repo.is_open"),
            Some(DangerLevel::High),
            ActionEffects {
                writes_refs: true,
                network: true,
                danger_level: DangerLevel::High,
                ..ActionEffects::default()
            },
            ConfirmPolicy::Always,
        ),
        host_action_spec(
            "remote.branch_list",
            "List Remote Branches",
            Some("repo.is_open"),
            None,
            ActionEffects::read_only(),
            ConfirmPolicy::Never,
        ),
        host_action_spec(
            "workspace.create",
            "Create Workspace",
            Some("always"),
            None,
            ActionEffects::default(),
            ConfirmPolicy::Never,
        ),
        host_action_spec(
            "workspace.add_repo",
            "Add Repo To Workspace",
            Some("always"),
            Some(DangerLevel::Low),
            ActionEffects {
                danger_level: DangerLevel::Low,
                ..ActionEffects::default()
            },
            ConfirmPolicy::Never,
        ),
        host_action_spec(
            "workspace.remove_repo",
            "Remove Repo From Workspace",
            Some("always"),
            Some(DangerLevel::Medium),
            ActionEffects {
                danger_level: DangerLevel::Medium,
                ..ActionEffects::default()
            },
            ConfirmPolicy::OnDanger,
        ),
        host_action_spec(
            "workspace.switch",
            "Switch Workspace",
            Some("always"),
            None,
            ActionEffects::read_only(),
            ConfirmPolicy::Never,
        ),
        host_action_spec(
            "workspace.switch_repo",
            "Switch Workspace Repo",
            Some("always"),
            None,
            ActionEffects::read_only(),
            ConfirmPolicy::Never,
        ),
        host_action_spec(
            "workspace.refresh_all",
            "Refresh Workspace Repos",
            Some("always"),
            None,
            ActionEffects::read_only(),
            ConfirmPolicy::Never,
        ),
        host_action_spec(
            "workspace.fetch_all",
            "Fetch Workspace Repos",
            Some("always"),
            Some(DangerLevel::Low),
            ActionEffects {
                network: true,
                danger_level: DangerLevel::Low,
                ..ActionEffects::default()
            },
            ConfirmPolicy::Never,
        ),
        host_action_spec(
            "pr.detect_provider",
            "Detect PR Provider",
            Some("repo.is_open"),
            None,
            ActionEffects::read_only(),
            ConfirmPolicy::Never,
        ),
        host_action_spec(
            "pr.list",
            "List Pull Requests",
            Some("repo.is_open"),
            None,
            ActionEffects::read_only(),
            ConfirmPolicy::Never,
        ),
        host_action_spec(
            "pr.create_url",
            "Create Pull Request URL",
            Some("repo.is_open"),
            None,
            ActionEffects::read_only(),
            ConfirmPolicy::Never,
        ),
        host_action_spec(
            "pr.open",
            "Open Pull Request",
            Some("repo.is_open"),
            None,
            ActionEffects::read_only(),
            ConfirmPolicy::Never,
        ),
        host_action_spec(
            "pr.checkout",
            "Checkout Pull Request",
            Some("repo.is_open"),
            Some(DangerLevel::Medium),
            ActionEffects {
                writes_refs: true,
                writes_worktree: true,
                network: true,
                danger_level: DangerLevel::Medium,
                ..ActionEffects::default()
            },
            ConfirmPolicy::OnDanger,
        ),
        host_action_spec(
            "stack.create",
            "Create Branch Stack",
            Some("repo.is_open"),
            None,
            ActionEffects::read_only(),
            ConfirmPolicy::Never,
        ),
        host_action_spec(
            "stack.detect",
            "Detect Branch Stack",
            Some("repo.is_open"),
            None,
            ActionEffects::read_only(),
            ConfirmPolicy::Never,
        ),
        host_action_spec(
            "stack.restack",
            "Restack Branch Stack",
            Some("repo.is_open"),
            Some(DangerLevel::High),
            ActionEffects {
                writes_refs: true,
                writes_worktree: true,
                danger_level: DangerLevel::High,
                ..ActionEffects::default()
            },
            ConfirmPolicy::Always,
        ),
        host_action_spec(
            "stack.restack_branch",
            "Restack Single Branch",
            Some("repo.is_open"),
            Some(DangerLevel::High),
            ActionEffects {
                writes_refs: true,
                writes_worktree: true,
                danger_level: DangerLevel::High,
                ..ActionEffects::default()
            },
            ConfirmPolicy::Always,
        ),
        host_action_spec(
            "journal.restore_ref",
            "Restore Ref From Journal",
            Some("repo.is_open"),
            Some(DangerLevel::High),
            ActionEffects::mutating_refs(),
            ConfirmPolicy::Always,
        ),
        host_action_spec(
            "journal.recover_operation",
            "Recover Journal Operation",
            Some("repo.is_open"),
            Some(DangerLevel::High),
            ActionEffects::mutating_refs(),
            ConfirmPolicy::Always,
        ),
        host_action_spec(
            "recovery.restore_ref",
            "Recovery Restore Ref",
            Some("repo.is_open"),
            Some(DangerLevel::High),
            ActionEffects::mutating_refs(),
            ConfirmPolicy::Always,
        ),
        host_action_spec(
            "recovery.create_branch_from_backup",
            "Create Branch From Backup",
            Some("repo.is_open"),
            Some(DangerLevel::Medium),
            ActionEffects {
                writes_refs: true,
                danger_level: DangerLevel::Medium,
                ..ActionEffects::default()
            },
            ConfirmPolicy::OnDanger,
        ),
        host_action_spec(
            "recovery.create_branch_from_reflog",
            "Create Branch From Reflog",
            Some("repo.is_open"),
            Some(DangerLevel::Medium),
            ActionEffects {
                writes_refs: true,
                danger_level: DangerLevel::Medium,
                ..ActionEffects::default()
            },
            ConfirmPolicy::OnDanger,
        ),
        host_action_spec(
            "plugin.discover",
            "Discover Registry Plugins",
            Some("always"),
            None,
            ActionEffects::read_only(),
            ConfirmPolicy::Never,
        ),
        host_action_spec(
            "plugin.marketplace",
            "Open Plugin Marketplace",
            Some("always"),
            None,
            ActionEffects::read_only(),
            ConfirmPolicy::Never,
        ),
        host_action_spec(
            "plugin.install",
            "Install Plugin",
            Some("always"),
            Some(DangerLevel::Low),
            ActionEffects {
                writes_worktree: true,
                danger_level: DangerLevel::Low,
                ..ActionEffects::default()
            },
            ConfirmPolicy::Never,
        ),
        host_action_spec(
            "plugin.install_registry",
            "Install Registry Plugin",
            Some("always"),
            Some(DangerLevel::Low),
            ActionEffects {
                writes_worktree: true,
                danger_level: DangerLevel::Low,
                ..ActionEffects::default()
            },
            ConfirmPolicy::Never,
        ),
        host_action_spec(
            "plugin.update",
            "Update Plugin",
            Some("always"),
            Some(DangerLevel::Medium),
            ActionEffects {
                writes_worktree: true,
                danger_level: DangerLevel::Medium,
                ..ActionEffects::default()
            },
            ConfirmPolicy::OnDanger,
        ),
        host_action_spec(
            "plugin.enable",
            "Enable Plugin",
            Some("always"),
            Some(DangerLevel::Low),
            ActionEffects {
                writes_worktree: true,
                danger_level: DangerLevel::Low,
                ..ActionEffects::default()
            },
            ConfirmPolicy::Never,
        ),
        host_action_spec(
            "plugin.disable",
            "Disable Plugin",
            Some("always"),
            Some(DangerLevel::Low),
            ActionEffects {
                writes_worktree: true,
                danger_level: DangerLevel::Low,
                ..ActionEffects::default()
            },
            ConfirmPolicy::Never,
        ),
        host_action_spec(
            "plugin.remove",
            "Remove Plugin",
            Some("always"),
            Some(DangerLevel::High),
            ActionEffects {
                writes_worktree: true,
                danger_level: DangerLevel::High,
                ..ActionEffects::default()
            },
            ConfirmPolicy::Always,
        ),
    ]
}

fn parse_command_line(line: &str) -> Result<ConsoleCommand, String> {
    let tokens = tokenize(line)?;
    let Some(command) = tokens.first().map(String::as_str) else {
        return Ok(ConsoleCommand::Empty);
    };

    match command {
        "help" => Ok(ConsoleCommand::Help),
        "show" => Ok(ConsoleCommand::Show),
        "actions" => Ok(ConsoleCommand::Actions),
        "ops" => Ok(ConsoleCommand::Ops),
        "refresh" => Ok(ConsoleCommand::Refresh),
        "quit" | "exit" => Ok(ConsoleCommand::Quit),
        "open" => {
            let path = join_tail(&tokens, 1)?;
            Ok(ConsoleCommand::Open { path })
        }
        "panel" => {
            let raw = tokens.get(1).ok_or_else(|| {
                "usage: panel <status|history|branches|tags|compare|diagnostics>".to_string()
            })?;
            let panel = PanelKind::parse(raw).ok_or_else(|| {
                "panel must be one of: status, history, branches, tags, compare, diagnostics"
                    .to_string()
            })?;
            Ok(ConsoleCommand::Panel { panel })
        }
        "run" => {
            let (run_tokens, confirmed) = extract_confirm_flags(&tokens[1..]);
            let target = run_tokens
                .first()
                .cloned()
                .ok_or_else(|| "usage: run [--confirm] <action_or_op> [args...]".to_string())?;
            let args = run_tokens[1..].to_vec();
            Ok(ConsoleCommand::Run {
                target,
                args,
                confirmed,
            })
        }
        "select" => {
            let target = match tokens.get(1).map(String::as_str) {
                Some("file") => SelectionTarget::File,
                Some("commit") => SelectionTarget::Commit,
                Some("branch") => SelectionTarget::Branch,
                Some("plugin") => SelectionTarget::Plugin,
                _ => return Err("usage: select <file|commit|branch|plugin> <value>".to_string()),
            };
            let value = join_tail(&tokens, 2)?;
            Ok(ConsoleCommand::Select { target, value })
        }
        "plugin" => {
            let (plugin_tokens, confirmed) = extract_confirm_flags(&tokens[1..]);
            let subcommand = plugin_tokens.first().map(String::as_str).ok_or_else(|| {
                "usage: plugin <list|discover|marketplace|install|install-registry|update|enable|disable|remove> ..."
                    .to_string()
            })?;
            let op = match subcommand {
                "list" => PluginOp::List,
                "discover" => PluginOp::Discover {
                    registry_path: join_tail_optional(&plugin_tokens, 1),
                },
                "marketplace" => PluginOp::Marketplace {
                    registry_path: join_tail_optional(&plugin_tokens, 1),
                },
                "install" => PluginOp::Install {
                    package_dir: join_tail(&plugin_tokens, 1)?,
                },
                "install-registry" => PluginOp::InstallRegistry {
                    plugin_id: plugin_tokens.get(1).cloned().ok_or_else(|| {
                        "usage: plugin install-registry <plugin_id> [registry_path]".to_string()
                    })?,
                    registry_path: join_tail_optional(&plugin_tokens, 2),
                },
                "update" => PluginOp::Update {
                    plugin_id: plugin_tokens.get(1).cloned().ok_or_else(|| {
                        "usage: plugin update <plugin_id> [registry_path]".to_string()
                    })?,
                    registry_path: join_tail_optional(&plugin_tokens, 2),
                },
                "enable" => PluginOp::Enable {
                    plugin_id: join_tail_optional(&plugin_tokens, 1),
                },
                "disable" => PluginOp::Disable {
                    plugin_id: join_tail_optional(&plugin_tokens, 1),
                },
                "remove" => PluginOp::Remove {
                    plugin_id: join_tail_optional(&plugin_tokens, 1),
                },
                _ => {
                    return Err("plugin must be one of: list, discover, marketplace, install, install-registry, update, enable, disable, remove".to_string());
                }
            };
            Ok(ConsoleCommand::Plugin { op, confirmed })
        }
        _ => Err(format!("unknown command `{command}`")),
    }
}

fn tokenize(line: &str) -> Result<Vec<String>, String> {
    let mut tokens = Vec::new();
    let mut current = String::new();
    let mut chars = line.trim().chars().peekable();
    let mut in_single = false;
    let mut in_double = false;

    while let Some(ch) = chars.next() {
        match ch {
            '\'' if !in_double => in_single = !in_single,
            '"' if !in_single => in_double = !in_double,
            '\\' if !in_single => {
                if let Some(next) = chars.next() {
                    current.push(next);
                }
            }
            c if c.is_whitespace() && !in_single && !in_double => {
                if !current.is_empty() {
                    tokens.push(std::mem::take(&mut current));
                }
            }
            _ => current.push(ch),
        }
    }

    if in_single || in_double {
        return Err("unterminated quote".to_string());
    }
    if !current.is_empty() {
        tokens.push(current);
    }
    Ok(tokens)
}

fn join_tail(tokens: &[String], start: usize) -> Result<String, String> {
    if start >= tokens.len() {
        Err("missing argument".to_string())
    } else {
        Ok(tokens[start..].join(" "))
    }
}

fn join_tail_optional(tokens: &[String], start: usize) -> Option<String> {
    if start >= tokens.len() {
        None
    } else {
        Some(tokens[start..].join(" "))
    }
}

fn extract_confirm_flags(tokens: &[String]) -> (Vec<String>, bool) {
    let mut confirmed = false;
    let filtered = tokens
        .iter()
        .filter_map(|token| match token.as_str() {
            "--confirm" | "-y" => {
                confirmed = true;
                None
            }
            _ => Some(token.clone()),
        })
        .collect::<Vec<_>>();
    (filtered, confirmed)
}

fn resolve_path(base: &Path, raw: &str) -> PathBuf {
    let path = PathBuf::from(raw);
    if path.is_absolute() {
        path
    } else {
        base.join(path)
    }
}

fn normalize_repo_path(repo_root: &Path, raw: &str) -> String {
    let path = PathBuf::from(raw);
    let candidate = if path.is_absolute() {
        path
    } else {
        repo_root.join(path)
    };

    candidate
        .strip_prefix(repo_root)
        .unwrap_or(candidate.as_path())
        .to_string_lossy()
        .replace('\\', "/")
}

fn help_text() -> String {
    [
        "Commands",
        "help",
        "open <path>",
        "panel <status|history|branches|tags|compare|diagnostics>",
        "show",
        "actions",
        "ops",
        "run [--confirm] <action_or_op> [args...]",
        "run <action_or_op> [args...] --confirm",
        "select file <path>",
        "select commit <oid>",
        "select branch <name>",
        "select plugin <id>",
        "refresh",
        "plugin <list|discover|install|install-registry|enable|disable|remove> ...",
        "quit",
        "",
        "Notes",
        "`actions` lists all action ids from the registered UI/action surface.",
        "`ops` lists the full direct job/host op catalog grouped by feature area.",
        "`run ...` can execute either an action id or a direct op.",
        "",
        "Examples",
        "open .",
        "panel history",
        "run history.page 0 20",
        "select commit <oid>",
        "run diff.commit <oid>",
        "select file Cargo.toml",
        "run index.stage_selected",
        "run index.stage_hunk Cargo.toml 0",
        "panel diagnostics",
        "plugin list",
        "select plugin sample_status",
        "run plugin.disable",
        "run branch.create feature/console-runner",
        "run --confirm branch.delete feature/old",
        "run branch.delete feature/old --confirm",
        "run --confirm rebase.interactive main autosquash",
        "run diagnostics.repo_capabilities",
        "run --confirm plugin.remove sample_status",
    ]
    .join("\n")
}

fn ops_text() -> String {
    [
        "Direct Ops",
        "",
        "[repo]",
        "repo.open <path>",
        "status.refresh",
        "refs.refresh",
        "",
        "[auth]",
        "auth.status",
        "auth.login <host> <username> <token> [github|gitlab]",
        "auth.logout <host> [username]",
        "auth.seed_git <host> [username]",
        "",
        "[remotes]",
        "remote.refresh",
        "remote.add <name> <url>",
        "remote.remove <name>",
        "remote.rename <old> <new>",
        "remote.fetch [remote]",
        "remote.fetch_all",
        "remote.pull",
        "remote.push",
        "remote.push_set_upstream [remote] [branch]",
        "remote.push_force_with_lease [remote] [branch]",
        "remote.branch_list",
        "",
        "[workspaces]",
        "workspace.create [name]",
        "workspace.add_repo <path> [group]",
        "workspace.remove_repo <repo_id|path|name>",
        "workspace.switch [workspace_id|name]",
        "workspace.switch_repo <repo_id|path|name>",
        "workspace.refresh_all",
        "workspace.fetch_all",
        "workspace.persist [out_file]",
        "workspace.restore [in_file]",
        "",
        "[pull-requests]",
        "pr.detect_provider",
        "pr.list [base_branch]",
        "pr.create_url [base_branch] [head_branch] [title...]",
        "pr.open [url]",
        "pr.checkout <number> [local_branch]",
        "",
        "[branch-stacks]",
        "stack.create <name> <base_ref> <branch...>",
        "stack.detect [base_ref]",
        "stack.restack <stack_id|name>",
        "",
        "[history]",
        "history.page <offset> <limit> [author] [text] [hash_prefix]",
        "history.load_more",
        "history.search <author> <text> [hash_prefix]",
        "history.clear_filter",
        "history.file <path> [offset] [limit]",
        "history.select_commit <oid>",
        "history.details <oid>",
        "blame.file <path>",
        "",
        "[diff]",
        "diff.worktree <path...>",
        "diff.index <path...>",
        "diff.commit <oid>",
        "compare.refs <base_ref> <head_ref>",
        "",
        "[index-and-commit]",
        "index.stage_paths <path...>",
        "index.unstage_paths <path...>",
        "index.stage_hunk <path> <hunk_index>",
        "index.stage_lines <path> <hunk_index> <line_index...>",
        "index.unstage_hunk <path> <hunk_index>",
        "index.unstage_lines <path> <hunk_index> <line_index...>",
        "file.discard <path...>",
        "file.discard_hunk <path> <hunk_index>",
        "file.discard_lines <path> <hunk_index> <line_index...>",
        "commit.create <message>",
        "commit.amend <message>",
        "",
        "[stash]",
        "stash.create <message>",
        "stash.list",
        "stash.apply <selector>",
        "stash.pop <selector>",
        "stash.drop <selector>",
        "",
        "[worktree-and-submodule]",
        "worktree.list",
        "worktree.create <path> <branch>",
        "worktree.remove <path>",
        "worktree.open <path>",
        "submodule.list",
        "submodule.init_update [path...]",
        "submodule.open <path>",
        "",
        "[branches-and-tags]",
        "branch.checkout <name>",
        "branch.create <name> [base_ref]",
        "branch.rename <old> <new>",
        "branch.delete <name>",
        "tag.create <name> [target]",
        "tag.delete <name>",
        "tag.checkout <name>",
        "",
        "[advanced-ops]",
        "merge.execute <source_ref> [ff|fast-forward|no-ff|squash]",
        "merge.abort",
        "cherry_pick.commit <oid>",
        "cherry_pick.abort",
        "revert.commit <oid>",
        "reset.refs <soft|mixed|hard> [target]",
        "rebase.plan.create <base_ref>",
        "rebase.plan.set_action <entry_index> <pick|reword|edit|squash|fixup|drop>",
        "rebase.plan.move <from_index> <to_index>",
        "rebase.plan.clear",
        "rebase.execute [autosquash]",
        "rebase.continue",
        "rebase.skip",
        "rebase.abort",
        "",
        "[conflicts]",
        "conflict.list",
        "conflict.focus <path>",
        "conflict.resolve.ours <path...>",
        "conflict.resolve.theirs <path...>",
        "conflict.mark_resolved <path...>",
        "conflict.continue",
        "conflict.abort",
        "",
        "[diagnostics]",
        "diagnostics.repo_capabilities",
        "diagnostics.lfs_status",
        "diagnostics.lfs_fetch",
        "diagnostics.lfs_pull",
        "",
        "[operations]",
        "ops.check_deps",
        "ops.dev_check",
        "release.notes [out_file] [channel]",
        "release.sign [artifact_dir]",
        "release.package_local [out_dir] [channel] [rollback_from]",
        "release.package [out_dir] [channel] [rollback_from]",
        "release.verify [out_dir] [channel] [rollback_from]",
        "verify.sprint22",
        "verify.sprint23 [out_dir]",
        "verify.sprint24 [out_dir] [channel] [rollback_from]",
        "",
        "[plugins]",
        "plugin.list",
        "plugin.discover [registry_path]",
        "plugin.marketplace [registry_path]",
        "plugin.install <package_dir>",
        "plugin.install_registry <plugin_id> [registry_path]",
        "plugin.update <plugin_id> [registry_path]",
        "plugin.enable [plugin_id]",
        "plugin.disable [plugin_id]",
        "plugin.remove [plugin_id]",
    ]
    .join("\n")
}

fn invalid_input_error(message: &str) -> UserFacingError {
    UserFacingError::with_category("Invalid input", message, None, ErrorCategory::Validation)
}

fn confirmation_required_error(action_id: &str, danger: DangerLevel) -> UserFacingError {
    UserFacingError::with_category(
        "Confirmation required",
        &format!(
            "`{action_id}` is a {}-risk operation. Re-run with `run --confirm {action_id} ...`.",
            danger_label(&danger)
        ),
        None,
        ErrorCategory::Validation,
    )
}

fn danger_label(danger: &DangerLevel) -> &'static str {
    match danger {
        DangerLevel::Low => "low",
        DangerLevel::Medium => "medium",
        DangerLevel::High => "high",
    }
}

fn translate_plugin_manager_error(error: PluginManagerError) -> UserFacingError {
    match error {
        PluginManagerError::Io(detail) => UserFacingError::with_category(
            "Plugin IO error",
            "Plugin filesystem operation failed.",
            Some(detail),
            ErrorCategory::System,
        ),
        PluginManagerError::InvalidManifest(detail) => UserFacingError::with_category(
            "Invalid plugin package",
            "Plugin manifest is invalid.",
            Some(detail),
            ErrorCategory::Validation,
        ),
        PluginManagerError::InvalidRegistry(detail) => UserFacingError::with_category(
            "Invalid plugin registry",
            "Plugin registry index is invalid.",
            Some(detail),
            ErrorCategory::Validation,
        ),
        PluginManagerError::UnsupportedSource(detail) => UserFacingError::with_category(
            "Unsupported plugin source",
            "Registry or package source is not supported by this host build.",
            Some(detail),
            ErrorCategory::Validation,
        ),
        PluginManagerError::IncompatiblePlugin {
            plugin_id,
            required_protocol,
            host_protocol,
        } => UserFacingError::with_category(
            "Incompatible plugin",
            &format!("Plugin `{plugin_id}` is not compatible with this host."),
            Some(format!(
                "required_protocol={required_protocol}, host_protocol={host_protocol}"
            )),
            ErrorCategory::Validation,
        ),
        PluginManagerError::AlreadyInstalled(plugin_id) => UserFacingError::with_category(
            "Plugin already installed",
            &format!("Plugin `{plugin_id}` is already installed."),
            None,
            ErrorCategory::Validation,
        ),
        PluginManagerError::NotInstalled(plugin_id) => UserFacingError::with_category(
            "Plugin not installed",
            &format!("Plugin `{plugin_id}` is not installed."),
            None,
            ErrorCategory::Validation,
        ),
        PluginManagerError::RegistryPluginNotFound(plugin_id) => UserFacingError::with_category(
            "Registry plugin not found",
            &format!("Plugin `{plugin_id}` is not present in the selected registry."),
            None,
            ErrorCategory::Validation,
        ),
    }
}

fn translate_operational_error(detail: String) -> UserFacingError {
    UserFacingError::with_category(
        "Operational command failed",
        "Runtime operational command failed.",
        Some(detail),
        ErrorCategory::System,
    )
}

fn write_user_error<W: Write, E: Write>(
    output: &mut W,
    debug_output: &mut E,
    error: &UserFacingError,
) -> std::io::Result<()> {
    writeln!(
        output,
        "error [{}] {}: {}",
        error.correlation_id, error.title, error.message
    )?;
    writeln!(
        debug_output,
        "{}",
        serde_json::json!({
            "correlation_id": error.correlation_id,
            "category": format!("{:?}", error.category),
            "title": error.title,
            "message": error.message,
            "detail": error.detail,
        })
    )?;
    Ok(())
}

fn lock_for_op(op: &str, _args: &[String]) -> Result<JobLock, UserFacingError> {
    let lock = match op {
        "repo.open"
        | "status.refresh"
        | "refs.refresh"
        | "history.page"
        | "history.load_more"
        | "history.search"
        | "history.clear_filter"
        | "history.file"
        | "history.select_commit"
        | "history.details"
        | "blame.file"
        | "stash.list"
        | "worktree.list"
        | "worktree.open"
        | "submodule.list"
        | "submodule.open"
        | "diagnostics.repo_capabilities"
        | "diagnostics.lfs_status"
        | "remote.refresh"
        | "remote.branch_list"
        | "pr.detect_provider"
        | "pr.list"
        | "pr.create_url"
        | "pr.open"
        | "stack.create"
        | "stack.detect"
        | "diff.worktree"
        | "diff.index"
        | "diff.commit"
        | "compare.refs"
        | "conflict.list"
        | "conflict.focus" => JobLock::Read,
        "rebase.plan.set_action" | "rebase.plan.move" | "rebase.plan.clear" => JobLock::Read,
        "index.stage_paths"
        | "index.unstage_paths"
        | "index.stage_hunk"
        | "index.stage_lines"
        | "index.unstage_hunk"
        | "index.unstage_lines"
        | "file.discard_hunk"
        | "file.discard_lines"
        | "stash.create"
        | "stash.apply"
        | "stash.pop"
        | "submodule.init_update"
        | "conflict.resolve.ours"
        | "conflict.resolve.theirs"
        | "conflict.mark_resolved" => JobLock::IndexWrite,
        "diagnostics.lfs_fetch"
        | "diagnostics.lfs_pull"
        | "remote.fetch"
        | "remote.fetch_all"
        | "remote.pull"
        | "remote.push"
        | "remote.push_set_upstream"
        | "remote.push_force_with_lease" => JobLock::Network,
        "commit.create"
        | "commit.amend"
        | "worktree.create"
        | "worktree.remove"
        | "stash.drop"
        | "merge.execute"
        | "merge.abort"
        | "cherry_pick.commit"
        | "cherry_pick.abort"
        | "revert.commit"
        | "rebase.plan.create"
        | "rebase.execute"
        | "rebase.continue"
        | "rebase.skip"
        | "rebase.abort"
        | "branch.checkout"
        | "branch.create"
        | "branch.rename"
        | "branch.delete"
        | "tag.create"
        | "tag.delete"
        | "tag.checkout"
        | "remote.add"
        | "remote.remove"
        | "remote.rename"
        | "stack.restack"
        | "stack.restack_branch"
        | "file.discard"
        | "conflict.continue"
        | "conflict.abort"
        | "recovery.restore_ref"
        | "recovery.create_branch_from_backup"
        | "recovery.create_branch_from_reflog" => JobLock::RefsWrite,
        "reset.refs" => JobLock::RefsWrite,
        _ => {
            return Err(UserFacingError::with_category(
                "Unsupported operation",
                &format!("Unknown job op `{op}`."),
                None,
                ErrorCategory::System,
            ));
        }
    };
    Ok(lock)
}

fn is_supported_direct_op(op: &str) -> bool {
    matches!(
        op,
        "repo.open"
            | "status.refresh"
            | "refs.refresh"
            | "history.page"
            | "history.load_more"
            | "history.search"
            | "history.clear_filter"
            | "history.file"
            | "history.select_commit"
            | "history.details"
            | "blame.file"
            | "index.stage_paths"
            | "index.unstage_paths"
            | "index.stage_hunk"
            | "index.stage_lines"
            | "index.unstage_hunk"
            | "index.unstage_lines"
            | "file.discard"
            | "file.discard_hunk"
            | "file.discard_lines"
            | "commit.create"
            | "commit.amend"
            | "stash.create"
            | "stash.list"
            | "stash.apply"
            | "stash.pop"
            | "stash.drop"
            | "worktree.list"
            | "worktree.create"
            | "worktree.remove"
            | "worktree.open"
            | "submodule.list"
            | "submodule.init_update"
            | "submodule.open"
            | "diagnostics.repo_capabilities"
            | "diagnostics.lfs_status"
            | "diagnostics.lfs_fetch"
            | "diagnostics.lfs_pull"
            | "diff.worktree"
            | "diff.index"
            | "diff.commit"
            | "compare.refs"
            | "conflict.focus"
            | "merge.execute"
            | "merge.abort"
            | "cherry_pick.commit"
            | "cherry_pick.abort"
            | "revert.commit"
            | "reset.refs"
            | "rebase.plan.create"
            | "rebase.plan.set_action"
            | "rebase.plan.move"
            | "rebase.plan.clear"
            | "rebase.execute"
            | "rebase.continue"
            | "rebase.skip"
            | "rebase.abort"
            | "conflict.list"
            | "conflict.resolve.ours"
            | "conflict.resolve.theirs"
            | "conflict.mark_resolved"
            | "conflict.continue"
            | "conflict.abort"
            | "branch.checkout"
            | "branch.create"
            | "branch.rename"
            | "branch.delete"
            | "tag.create"
            | "tag.delete"
            | "tag.checkout"
            | "remote.refresh"
            | "remote.add"
            | "remote.remove"
            | "remote.rename"
            | "remote.fetch"
            | "remote.fetch_all"
            | "remote.pull"
            | "remote.push"
            | "remote.push_set_upstream"
            | "remote.push_force_with_lease"
            | "remote.branch_list"
            | "stack.restack_branch"
            | "journal.open_entry"
            | "journal.copy_details"
            | "journal.export"
            | "journal.restore_ref"
            | "journal.recover_operation"
            | "journal.clear_old_entries"
            | "recovery.restore_ref"
            | "recovery.create_branch_from_backup"
            | "recovery.create_branch_from_reflog"
            | "plugin.list"
            | "plugin.discover"
            | "plugin.marketplace"
            | "plugin.install"
            | "plugin.install_registry"
            | "plugin.update"
            | "plugin.enable"
            | "plugin.disable"
            | "plugin.remove"
    )
}

fn is_replayable_op(op: &str) -> bool {
    matches!(
        op,
        "status.refresh"
            | "refs.refresh"
            | "history.page"
            | "history.search"
            | "history.file"
            | "history.select_commit"
            | "history.details"
            | "blame.file"
            | "stash.list"
            | "worktree.list"
            | "submodule.list"
            | "diagnostics.repo_capabilities"
            | "diff.worktree"
            | "diff.index"
            | "diff.commit"
            | "compare.refs"
    )
}

fn render_text_diff(id: &str, content: String) -> DiffState {
    DiffState {
        source: Some(DiffSource::Commit {
            oid: id.to_string(),
        }),
        descriptor: None,
        load_request: None,
        chunks: Vec::new(),
        content: Some(content),
        hunks: Vec::new(),
        loading: false,
        error: None,
    }
}

fn render_plugin_list(
    installed: &[plugin_host::InstalledPluginInfo],
    plugins_root: &Path,
) -> String {
    if installed.is_empty() {
        return format!("plugins_root: {}\nplugins: <empty>", plugins_root.display());
    }

    let mut lines = vec![format!("plugins_root: {}", plugins_root.display())];
    for plugin in installed {
        lines.push(format!(
            "{} v{} enabled={} protocol={} perms={}",
            plugin.manifest.plugin_id,
            plugin.manifest.version,
            plugin.enabled,
            plugin.manifest.protocol_version,
            plugin.manifest.permissions.join(", ")
        ));
    }
    lines.join("\n")
}

fn render_discovered_plugin_list(
    discovered: &[plugin_host::DiscoverablePluginInfo],
    registry_path: &Path,
) -> String {
    if discovered.is_empty() {
        return format!("registry: {}\nplugins: <empty>", registry_path.display());
    }

    let mut lines = vec![format!("registry: {}", registry_path.display())];
    for plugin in discovered {
        let package_label = if let Some(manifest_url) = plugin.manifest_url.as_deref() {
            let entrypoint_url = plugin.entrypoint_url.as_deref().unwrap_or("<missing>");
            format!(
                "remote manifest={} entrypoint={}",
                manifest_url, entrypoint_url
            )
        } else {
            format!("package={}", plugin.package_dir.display())
        };
        lines.push(format!(
            "{} v{} channel={} {} signature={:?} perms={}",
            plugin.manifest.plugin_id,
            plugin.manifest.version,
            plugin.channel.as_deref().unwrap_or("stable"),
            package_label,
            plugin_signature_status_for_dir(&plugin.package_dir).0,
            plugin.manifest.permissions.join(", ")
        ));
        if let Some(summary) = plugin.summary.as_deref() {
            lines.push(format!("  summary: {summary}"));
        }
    }
    lines.join("\n")
}

fn render_plugin_marketplace_list(
    discovered: &[plugin_host::DiscoverablePluginInfo],
    installed: &[plugin_host::InstalledPluginInfo],
    registry_path: &Path,
) -> String {
    if discovered.is_empty() {
        return format!("marketplace: {}\nplugins: <empty>", registry_path.display());
    }

    let mut lines = vec![format!("marketplace: {}", registry_path.display())];
    for plugin in discovered {
        let installed_version = installed
            .iter()
            .find(|installed| installed.manifest.plugin_id == plugin.manifest.plugin_id)
            .map(|installed| installed.manifest.version.as_str());
        let update = installed_version
            .map(|version| version != plugin.manifest.version)
            .unwrap_or(false);
        let (signature_status, signature_note) =
            plugin_signature_status_for_dir(&plugin.package_dir);
        lines.push(format!(
            "{} v{} channel={} installed={} update={} signature={:?}",
            plugin.manifest.plugin_id,
            plugin.manifest.version,
            plugin.channel.as_deref().unwrap_or("stable"),
            installed_version.unwrap_or("<not installed>"),
            update,
            signature_status
        ));
        if let Some(note) = signature_note {
            lines.push(format!("  signature: {note}"));
        }
        if let Some(summary) = plugin.summary.as_deref() {
            lines.push(format!("  summary: {summary}"));
        }
        lines.push(format!(
            "  permissions: {}",
            plugin.manifest.permissions.join(", ")
        ));
    }
    lines.join("\n")
}

fn map_installed_plugins(
    installed: &[plugin_host::InstalledPluginInfo],
) -> Vec<InstalledPluginRecord> {
    installed
        .iter()
        .map(|plugin| InstalledPluginRecord {
            plugin_id: plugin.manifest.plugin_id.clone(),
            version: plugin.manifest.version.clone(),
            protocol_version: plugin.manifest.protocol_version.clone(),
            enabled: plugin.enabled,
            description: plugin.manifest.description.clone(),
            permissions: plugin.manifest.permissions.clone(),
            install_dir: plugin.install_dir.display().to_string(),
        })
        .collect()
}

fn map_plugin_security_records(
    installed: &[plugin_host::InstalledPluginInfo],
    actions: &[CatalogAction],
    plugins_root: &Path,
) -> Vec<PluginSecurityRecord> {
    map_plugin_security_records_with_updates(installed, actions, plugins_root, &[])
}

fn map_plugin_security_records_with_updates(
    installed: &[plugin_host::InstalledPluginInfo],
    actions: &[CatalogAction],
    plugins_root: &Path,
    discovered: &[plugin_host::DiscoverablePluginInfo],
) -> Vec<PluginSecurityRecord> {
    installed
        .iter()
        .map(|plugin| {
            let (signature_status, signature_note) = plugin_signature_status_for_dir(&plugin.install_dir);
            let signed_marker = plugin.install_dir.join("SIGNATURE").exists()
                || plugin.manifest.permissions.iter().any(|permission| permission == "signed");
            let signed = matches!(signature_status, PluginSignatureStatus::Verified)
                || signed_marker;
            let trust_level = if plugin.install_dir.starts_with(plugins_root.join("bundled")) {
                PluginTrustLevel::Bundled
            } else if matches!(signature_status, PluginSignatureStatus::Verified) {
                PluginTrustLevel::SignedCommunity
            } else if plugin.enabled {
                PluginTrustLevel::UnsignedLocal
            } else {
                PluginTrustLevel::ExperimentalSandboxed
            };
            let contributed_actions = actions
                .iter()
                .filter(|action| action.owner == plugin.manifest.plugin_id)
                .map(|action| action.spec.action_id.clone())
                .collect::<Vec<_>>();
            let mut warnings = Vec::new();
            if let Some(note) = signature_note {
                warnings.push(note);
            }
            if !matches!(signature_status, PluginSignatureStatus::Verified)
                && !matches!(trust_level, PluginTrustLevel::Bundled)
            {
                warnings.push(
                    "signature is not cryptographically verified: keep disabled until the source is trusted".to_string(),
                );
            }
            for permission in &plugin.manifest.permissions {
                if matches!(
                    permission.as_str(),
                    "write_repo" | "network" | "spawn_process" | "filesystem_write"
                ) {
                    warnings.push(format!("high-impact permission requested: {permission}"));
                }
            }
            if plugin.manifest.protocol_version != plugin_api::HOST_PLUGIN_PROTOCOL_VERSION {
                warnings.push(format!(
                    "protocol {} differs from host {}",
                    plugin.manifest.protocol_version,
                    plugin_api::HOST_PLUGIN_PROTOCOL_VERSION
                ));
            }
            let update_available = discovered
                .iter()
                .find(|candidate| candidate.manifest.plugin_id == plugin.manifest.plugin_id)
                .map(|candidate| candidate.manifest.version != plugin.manifest.version)
                .unwrap_or(false);
            let sandbox_mode = plugin_sandbox_mode(&plugin.manifest.permissions);
            PluginSecurityRecord {
                plugin_id: plugin.manifest.plugin_id.clone(),
                trust_level,
                signed,
                signature_status,
                sandbox_mode,
                permissions: plugin.manifest.permissions.clone(),
                contributed_actions,
                contributed_views: Vec::new(),
                warnings,
                update_available,
            }
        })
        .collect()
}

fn plugin_signature_status_for_dir(dir: &Path) -> (PluginSignatureStatus, Option<String>) {
    let manifest = dir.join("plugin.json");
    let signature = dir.join("plugin.sig");
    let public_key = dir.join("plugin.pub");
    if !signature.exists() && !dir.join("SIGNATURE").exists() {
        return (PluginSignatureStatus::Missing, None);
    }
    if !signature.exists() || !public_key.exists() {
        return (
            PluginSignatureStatus::PresentUnverified,
            Some("signature marker is present but plugin.sig/plugin.pub verification files are missing".to_string()),
        );
    }
    let output = std::process::Command::new("openssl")
        .args(["dgst", "-sha256", "-verify"])
        .arg(&public_key)
        .arg("-signature")
        .arg(&signature)
        .arg(&manifest)
        .output();
    match output {
        Ok(output) if output.status.success() => (PluginSignatureStatus::Verified, None),
        Ok(output) => {
            let stderr = String::from_utf8(output.stderr)
                .unwrap_or_else(|_| "openssl verification failed".to_string());
            (
                PluginSignatureStatus::Invalid,
                Some(format!("signature verification failed: {}", stderr.trim())),
            )
        }
        Err(err) => (
            PluginSignatureStatus::PresentUnverified,
            Some(format!("signature verification unavailable: {err}")),
        ),
    }
}

fn plugin_sandbox_mode(permissions: &[String]) -> String {
    if permissions
        .iter()
        .any(|permission| matches!(permission.as_str(), "spawn_process" | "filesystem_write"))
    {
        "process-isolated-high-impact".to_string()
    } else if permissions
        .iter()
        .any(|permission| matches!(permission.as_str(), "network" | "write_repo"))
    {
        "process-isolated-permission-gated".to_string()
    } else {
        "process-isolated-read-mostly".to_string()
    }
}

fn render_journal_summary(store: &StateStore) -> String {
    let mut lines = vec!["Journal Summary".to_string()];
    let entries = &store.snapshot().journal.entries;
    lines.push(format!("entries: {}", entries.len()));

    let running = entries
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
    lines.push(format!("running: {running}"));
    lines.push(format!("succeeded: {succeeded}"));
    lines.push(format!("failed: {failed}"));

    if !store.snapshot().plugins.is_empty() {
        let plugin_summary = store
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
            .join(", ");
        lines.push(format!("plugins: {plugin_summary}"));
    }

    lines.push("recent:".to_string());
    if entries.is_empty() {
        lines.push("<empty>".to_string());
    } else {
        for entry in entries.iter().rev().take(10) {
            let status = match entry.status {
                state_store::JournalStatus::Started => "running",
                state_store::JournalStatus::Succeeded => "ok",
                state_store::JournalStatus::Failed => "failed",
            };
            let duration = match (entry.started_at_ms, entry.finished_at_ms) {
                (start, Some(end)) if end >= start => format!("{}ms", end - start),
                _ => "-".to_string(),
            };
            let suffix = entry
                .error
                .as_deref()
                .map(|error| format!(" | error={error}"))
                .unwrap_or_default();
            let recovery = if entry.backup_refs.is_empty() {
                ""
            } else {
                " recovery=available"
            };
            lines.push(format!(
                "#{} {} {} duration={}{}{}",
                entry.id, status, entry.op, duration, recovery, suffix
            ));
        }
    }

    lines.join("\n")
}

fn format_ref_snapshot(label: &str, refs: &state_store::RefSnapshotSummary) -> String {
    format!(
        "{label}: head={} oid={} branches={} tags={} tracked_refs={}",
        refs.head.as_deref().unwrap_or("<none>"),
        refs.head_oid.as_deref().unwrap_or("<unknown>"),
        refs.branch_count,
        refs.tag_count,
        refs.refs.len()
    )
}

fn redact_params(params: &[String]) -> Vec<String> {
    params
        .iter()
        .map(|param| {
            let lower = param.to_lowercase();
            if lower.contains("token")
                || lower.contains("password")
                || lower.contains("secret")
                || lower.contains("credential")
            {
                "<redacted>".to_string()
            } else {
                param.clone()
            }
        })
        .collect()
}

fn explain_template_for_operation(op: &str) -> Option<ExplainTemplate> {
    match op {
        "reset.refs" => explain_template_for_action("reset.hard"),
        "recovery.restore_ref" | "recovery.create_branch_from_backup" => Some(explain(
            "recovery.restore_ref",
            "This restores a ref or creates a branch from a saved recovery point.",
            &[
                "git update-ref <ref> <oid>",
                "git branch <name> <backup-ref>",
            ],
            &["Restoring a ref moves that ref and can change what future checkouts see."],
            &["Recovery operations are journaled so the previous state can be inspected."],
        )),
        other => explain_template_for_action(other),
    }
}

fn view_to_owner(view_id: &str) -> Option<&'static str> {
    match view_id {
        "status.panel" => Some("status"),
        "history.panel" => Some("history"),
        "branches.panel" => Some("branches"),
        "tags.panel" => Some("tags"),
        "compare.panel" => Some("compare"),
        "diagnostics.panel" => Some("diagnostics"),
        _ => None,
    }
}

fn is_merge_mode(value: &str) -> bool {
    matches!(value, "ff" | "fast-forward" | "no-ff" | "squash")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn unique_temp_dir(label: &str) -> PathBuf {
        static COUNTER: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_nanos())
            .unwrap_or(0);
        let seq = COUNTER.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        std::env::temp_dir().join(format!("branchforge-console-runner-{label}-{nanos}-{seq}"))
    }

    fn workspace_repo_root() -> PathBuf {
        PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .parent()
            .and_then(Path::parent)
            .expect("workspace root")
            .to_path_buf()
    }

    fn test_config(root: &Path) -> ConsoleRunnerConfig {
        ConsoleRunnerConfig {
            cwd: root.to_path_buf(),
            plugins_root: root.join("plugins"),
            auth_metadata_path: Some(root.join("auth/accounts.json")),
            auth_file_store: Some(root.join("auth/tokens")),
            github_api_base: None,
            gitlab_api_base: None,
            auto_render: true,
        }
    }

    fn init_repo(label: &str) -> PathBuf {
        let repo_dir = unique_temp_dir(label);
        assert!(std::fs::create_dir_all(&repo_dir).is_ok());
        assert!(git_service::run_git(&repo_dir, &["init"]).is_ok());
        assert!(
            git_service::run_git(&repo_dir, &["config", "user.email", "dev@example.com"]).is_ok()
        );
        assert!(git_service::run_git(&repo_dir, &["config", "user.name", "Dev User"]).is_ok());
        repo_dir
    }

    fn git_lfs_available() -> bool {
        std::process::Command::new("git")
            .args(["lfs", "version"])
            .output()
            .map(|output| output.status.success())
            .unwrap_or(false)
    }

    fn init_lfs_runtime_repo(label: &str) -> Option<(PathBuf, String)> {
        if !git_lfs_available() {
            return None;
        }

        let root = unique_temp_dir(label);
        let origin = root.join("origin.git");
        let source = root.join("source");
        let clone = root.join("clone");
        let payload = "branchforge-lfs-runtime\n".repeat(64);

        assert!(std::fs::create_dir_all(&source).is_ok());
        assert!(git_service::run_git(&source, &["init"]).is_ok());
        assert!(
            git_service::run_git(&source, &["config", "user.email", "dev@example.com"]).is_ok()
        );
        assert!(git_service::run_git(&source, &["config", "user.name", "Dev User"]).is_ok());
        assert!(git_service::run_git(&source, &["lfs", "install", "--local"]).is_ok());
        assert!(git_service::run_git(&source, &["lfs", "track", "*.bin"]).is_ok());
        assert!(std::fs::write(source.join("payload.bin"), &payload).is_ok());
        assert!(
            git_service::stage_paths(
                &source,
                &[".gitattributes".to_string(), "payload.bin".to_string()],
            )
            .is_ok()
        );
        assert!(git_service::commit_create(&source, "add lfs payload").is_ok());

        assert!(
            std::process::Command::new("git")
                .args(["init", "--bare", origin.to_string_lossy().as_ref()])
                .output()
                .map(|output| output.status.success())
                .unwrap_or(false)
        );
        assert!(
            git_service::run_git(
                &source,
                &["remote", "add", "origin", origin.to_string_lossy().as_ref()],
            )
            .is_ok()
        );
        assert!(git_service::run_git(&source, &["push", "-u", "origin", "HEAD"]).is_ok());

        let branch = git_service::run_git(&source, &["branch", "--show-current"])
            .ok()
            .and_then(|out| String::from_utf8(out.stdout).ok())
            .map(|text| text.trim().to_string())
            .filter(|value| !value.is_empty())
            .unwrap_or_else(|| "master".to_string());
        assert!(
            std::process::Command::new("git")
                .args([
                    "--git-dir",
                    origin.to_string_lossy().as_ref(),
                    "symbolic-ref",
                    "HEAD",
                    &format!("refs/heads/{branch}"),
                ])
                .output()
                .map(|output| output.status.success())
                .unwrap_or(false)
        );

        assert!(
            std::process::Command::new("git")
                .env("GIT_LFS_SKIP_SMUDGE", "1")
                .args([
                    "clone",
                    origin.to_string_lossy().as_ref(),
                    clone.to_string_lossy().as_ref(),
                ])
                .output()
                .map(|output| output.status.success())
                .unwrap_or(false)
        );
        assert!(git_service::run_git(&clone, &["lfs", "install", "--local"]).is_ok());

        Some((clone, payload))
    }

    fn create_plugin_package(root: &Path, plugin_id: &str) -> PathBuf {
        let package_dir = root.join(format!("pkg-{plugin_id}"));
        assert!(std::fs::create_dir_all(&package_dir).is_ok());
        assert!(
            std::fs::write(
                package_dir.join("plugin_bin"),
                "#!/usr/bin/env sh\nexit 0\n"
            )
            .is_ok()
        );
        let manifest = plugin_api::PluginManifestV1 {
            manifest_version: plugin_api::PLUGIN_MANIFEST_VERSION_V1.to_string(),
            plugin_id: plugin_id.to_string(),
            version: "0.1.0".to_string(),
            protocol_version: plugin_api::HOST_PLUGIN_PROTOCOL_VERSION.to_string(),
            entrypoint: "plugin_bin".to_string(),
            description: Some(format!("{plugin_id} plugin")),
            permissions: vec!["read_state".to_string()],
        };
        let raw = serde_json::to_string_pretty(&manifest).unwrap_or_else(|_| "{}".to_string());
        assert!(std::fs::write(package_dir.join("plugin.json"), raw).is_ok());
        package_dir
    }

    fn create_plugin_registry(root: &Path, plugin_id: &str, package_dir: &Path) -> PathBuf {
        let registry_dir = root.join("plugin_registry");
        assert!(std::fs::create_dir_all(&registry_dir).is_ok());
        let relative_package = package_dir
            .strip_prefix(root)
            .unwrap_or(package_dir)
            .to_string_lossy()
            .to_string();
        assert!(
            std::fs::write(
                registry_dir.join("registry.json"),
                serde_json::json!({
                    "registry_version": "1",
                    "plugins": [{
                        "plugin_id": plugin_id,
                        "package_dir": format!("../{relative_package}"),
                        "summary": format!("{plugin_id} registry plugin"),
                        "channel": "stable"
                    }]
                })
                .to_string(),
            )
            .is_ok()
        );
        registry_dir
    }

    fn build_sample_external_plugin() -> PathBuf {
        let package_dir = workspace_repo_root().join("external_plugins/sample_plugin");
        let status = std::process::Command::new("cargo")
            .args(["build", "--manifest-path"])
            .arg(package_dir.join("Cargo.toml"))
            .status()
            .expect("build sample external plugin");
        assert!(status.success());
        package_dir
    }

    fn spawn_json_http_server(responses: Vec<String>) -> (String, std::thread::JoinHandle<()>) {
        let listener = std::net::TcpListener::bind("127.0.0.1:0").expect("bind test server");
        let addr = listener.local_addr().expect("server addr");
        let handle = std::thread::spawn(move || {
            for response in responses {
                let Ok((mut stream, _)) = listener.accept() else {
                    return;
                };
                let mut buffer = [0_u8; 2048];
                let _ = std::io::Read::read(&mut stream, &mut buffer);
                let payload = format!(
                    "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
                    response.len(),
                    response
                );
                let _ = std::io::Write::write_all(&mut stream, payload.as_bytes());
            }
        });
        (format!("http://{addr}"), handle)
    }

    #[test]
    fn parses_run_with_confirm_and_quotes() {
        let command = parse_command_line("run --confirm branch.delete \"feature/old branch\"")
            .expect("parse");
        assert_eq!(
            command,
            ConsoleCommand::Run {
                target: "branch.delete".to_string(),
                args: vec!["feature/old branch".to_string()],
                confirmed: true,
            }
        );
    }

    #[test]
    fn parses_run_with_postfix_confirm() {
        let command = parse_command_line("run branch.delete feature/old --confirm").expect("parse");
        assert_eq!(
            command,
            ConsoleCommand::Run {
                target: "branch.delete".to_string(),
                args: vec!["feature/old".to_string()],
                confirmed: true,
            }
        );
    }

    #[test]
    fn parses_select_file_with_spaces() {
        let command = parse_command_line("select file \"docs/with spaces.md\"").expect("parse");
        assert_eq!(
            command,
            ConsoleCommand::Select {
                target: SelectionTarget::File,
                value: "docs/with spaces.md".to_string(),
            }
        );
    }

    #[test]
    fn parses_select_plugin() {
        let command = parse_command_line("select plugin sample_status").expect("parse");
        assert_eq!(
            command,
            ConsoleCommand::Select {
                target: SelectionTarget::Plugin,
                value: "sample_status".to_string(),
            }
        );
    }

    #[test]
    fn parses_plugin_install_command() {
        let command =
            parse_command_line("plugin install external_plugins/sample_plugin").expect("parse");
        assert_eq!(
            command,
            ConsoleCommand::Plugin {
                op: PluginOp::Install {
                    package_dir: "external_plugins/sample_plugin".to_string(),
                },
                confirmed: false,
            }
        );
    }

    #[test]
    fn parses_plugin_discover_command() {
        let command = parse_command_line("plugin discover plugin_registry").expect("parse");
        assert_eq!(
            command,
            ConsoleCommand::Plugin {
                op: PluginOp::Discover {
                    registry_path: Some("plugin_registry".to_string()),
                },
                confirmed: false,
            }
        );
    }

    #[test]
    fn resolve_plugin_registry_path_preserves_url_sources() {
        let root = unique_temp_dir("plugin-registry-url");
        assert!(std::fs::create_dir_all(&root).is_ok());
        let runner = ConsoleRunner::new(test_config(&root));

        let resolved =
            runner.resolve_plugin_registry_path(Some("http://127.0.0.1:3000/registry.json"));
        assert_eq!(
            resolved,
            PathBuf::from("http://127.0.0.1:3000/registry.json")
        );

        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn reset_hard_preview_describes_ref_worktree_and_command() {
        let repo_dir = init_repo("reset-preview");
        assert!(std::fs::write(repo_dir.join("README.md"), "base\n").is_ok());
        assert!(git_service::stage_paths(&repo_dir, &["README.md".to_string()]).is_ok());
        assert!(git_service::commit_create(&repo_dir, "base").is_ok());

        let mut runner = ConsoleRunner::new(test_config(&repo_dir));
        assert!(
            runner
                .execute(ConsoleCommand::Open {
                    path: repo_dir.to_string_lossy().to_string(),
                })
                .is_ok()
        );

        let preview = runner
            .preview_operation("reset.hard", &["HEAD".to_string()])
            .expect("preview");
        assert_eq!(preview.operation, "reset.hard");
        assert_eq!(preview.worktree_impact.level, ImpactLevel::Destructive);
        assert_eq!(preview.index_impact.level, ImpactLevel::Destructive);
        assert!(!preview.affected_refs.is_empty());
        assert!(
            preview
                .git_commands
                .iter()
                .any(|command| command.contains("git reset --hard"))
        );

        let _ = std::fs::remove_dir_all(&repo_dir);
    }

    #[test]
    fn branch_delete_preview_marks_unmerged_branch_as_dangerous() {
        let repo_dir = init_repo("branch-delete-preview");
        assert!(std::fs::write(repo_dir.join("README.md"), "base\n").is_ok());
        assert!(git_service::stage_paths(&repo_dir, &["README.md".to_string()]).is_ok());
        assert!(git_service::commit_create(&repo_dir, "base").is_ok());
        let base_branch = git_service::repo_open(&repo_dir)
            .expect("repo")
            .head
            .expect("head branch");
        assert!(git_service::create_branch(&repo_dir, "feature/unmerged").is_ok());
        assert!(git_service::checkout_branch(&repo_dir, "feature/unmerged").is_ok());
        assert!(std::fs::write(repo_dir.join("feature.txt"), "feature\n").is_ok());
        assert!(git_service::stage_paths(&repo_dir, &["feature.txt".to_string()]).is_ok());
        assert!(git_service::commit_create(&repo_dir, "feature").is_ok());
        assert!(git_service::checkout_branch(&repo_dir, &base_branch).is_ok());

        let mut runner = ConsoleRunner::new(test_config(&repo_dir));
        assert!(
            runner
                .execute(ConsoleCommand::Open {
                    path: repo_dir.to_string_lossy().to_string(),
                })
                .is_ok()
        );

        let preview = runner
            .preview_operation("branch.delete", &["feature/unmerged".to_string()])
            .expect("preview");
        assert!(
            preview
                .warnings
                .iter()
                .any(|warning| warning.level == PreviewWarningLevel::Danger)
        );
        assert!(preview.summary.contains("may contain commits not merged"));

        let _ = std::fs::remove_dir_all(&repo_dir);
    }

    #[test]
    fn high_and_always_confirm_actions_have_explain_templates() {
        let root = unique_temp_dir("explain-coverage");
        assert!(std::fs::create_dir_all(&root).is_ok());
        let runner = ConsoleRunner::new(test_config(&root));
        let missing = runner
            .action_catalog_items()
            .into_iter()
            .filter(|item| {
                matches!(item.confirm_policy, ConfirmPolicy::Always)
                    || (matches!(item.confirm_policy, ConfirmPolicy::OnDanger)
                        && matches!(item.danger, DangerLevel::High))
            })
            .filter(|item| item.explain.is_none())
            .map(|item| item.action_id)
            .collect::<Vec<_>>();

        assert!(missing.is_empty(), "missing explain templates: {missing:?}");
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn parses_plugin_install_registry_command() {
        let command = parse_command_line("plugin install-registry sample_status plugin_registry")
            .expect("parse");
        assert_eq!(
            command,
            ConsoleCommand::Plugin {
                op: PluginOp::InstallRegistry {
                    plugin_id: "sample_status".to_string(),
                    registry_path: Some("plugin_registry".to_string()),
                },
                confirmed: false,
            }
        );
    }

    #[test]
    fn parses_plugin_disable_without_id() {
        let command = parse_command_line("plugin disable").expect("parse");
        assert_eq!(
            command,
            ConsoleCommand::Plugin {
                op: PluginOp::Disable { plugin_id: None },
                confirmed: false,
            }
        );
    }

    #[test]
    fn parses_plugin_remove_with_postfix_confirm() {
        let command = parse_command_line("plugin remove sample_status --confirm").expect("parse");
        assert_eq!(
            command,
            ConsoleCommand::Plugin {
                op: PluginOp::Remove {
                    plugin_id: Some("sample_status".to_string()),
                },
                confirmed: true,
            }
        );
    }

    #[test]
    fn ops_text_lists_advanced_and_productivity_features() {
        let ops = ops_text();
        assert!(ops.contains("history.search <author> <text> [hash_prefix]"));
        assert!(ops.contains("remote.fetch_all"));
        assert!(ops.contains("remote.push_force_with_lease [remote] [branch]"));
        assert!(ops.contains("workspace.add_repo <path> [group]"));
        assert!(ops.contains("pr.create_url [base_branch] [head_branch] [title...]"));
        assert!(ops.contains("stack.restack <stack_id|name>"));
        assert!(ops.contains("commit.amend <message>"));
        assert!(ops.contains("stash.list"));
        assert!(ops.contains("worktree.create <path> <branch>"));
        assert!(ops.contains("submodule.init_update [path...]"));
        assert!(ops.contains("merge.execute <source_ref> [ff|fast-forward|no-ff|squash]"));
        assert!(ops.contains("index.stage_lines <path> <hunk_index> <line_index...>"));
        assert!(ops.contains("index.unstage_lines <path> <hunk_index> <line_index...>"));
        assert!(ops.contains("file.discard_lines <path> <hunk_index> <line_index...>"));
        assert!(ops.contains("rebase.plan.create <base_ref>"));
        assert!(
            ops.contains(
                "rebase.plan.set_action <entry_index> <pick|reword|edit|squash|fixup|drop>"
            )
        );
        assert!(ops.contains("rebase.plan.move <from_index> <to_index>"));
        assert!(ops.contains("rebase.plan.clear"));
        assert!(ops.contains("conflict.focus <path>"));
        assert!(ops.contains("conflict.resolve.ours <path...>"));
        assert!(ops.contains("diagnostics.lfs_status"));
        assert!(ops.contains("diagnostics.lfs_fetch"));
        assert!(ops.contains("diagnostics.lfs_pull"));
        assert!(ops.contains("plugin.discover [registry_path]"));
        assert!(ops.contains("plugin.install_registry <plugin_id> [registry_path]"));
        assert!(ops.contains("plugin.remove [plugin_id]"));
        assert!(ops.contains("verify.sprint22"));
        assert!(ops.contains("verify.sprint23 [out_dir]"));
        assert!(ops.contains("verify.sprint24 [out_dir] [channel] [rollback_from]"));
    }

    #[test]
    fn workspace_ops_persist_and_refresh_repo_summaries() {
        let root = unique_temp_dir("workspace-ops");
        let repo_dir = root.join("repo");
        assert!(std::fs::create_dir_all(&repo_dir).is_ok());
        assert!(git_service::run_git(&repo_dir, &["init"]).is_ok());
        assert!(
            git_service::run_git(&repo_dir, &["config", "user.email", "dev@example.com"]).is_ok()
        );
        assert!(git_service::run_git(&repo_dir, &["config", "user.name", "Dev User"]).is_ok());
        assert!(std::fs::write(repo_dir.join("README.md"), "base\n").is_ok());
        assert!(git_service::stage_paths(&repo_dir, &["README.md".to_string()]).is_ok());
        assert!(git_service::commit_create(&repo_dir, "base").is_ok());

        let mut runner = ConsoleRunner::new(test_config(&root));
        assert!(
            runner
                .execute(ConsoleCommand::Run {
                    target: "workspace.create".to_string(),
                    args: vec!["Team".to_string()],
                    confirmed: false,
                })
                .is_ok()
        );
        assert!(
            runner
                .execute(ConsoleCommand::Run {
                    target: "workspace.add_repo".to_string(),
                    args: vec![repo_dir.to_string_lossy().to_string()],
                    confirmed: false,
                })
                .is_ok()
        );
        assert!(
            runner
                .execute(ConsoleCommand::Run {
                    target: "workspace.refresh_all".to_string(),
                    args: Vec::new(),
                    confirmed: false,
                })
                .is_ok()
        );
        let snapshot = runner.store.snapshot();
        assert_eq!(snapshot.workspace.workspaces.len(), 1);
        assert_eq!(snapshot.workspace.workspaces[0].repos.len(), 1);
        assert!(
            !snapshot.workspace.workspaces[0].repos[0]
                .status_summary
                .dirty
        );
        assert!(workspace_store_path_for(&root).exists());

        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn pr_ops_detect_provider_and_create_provider_url() {
        let root = unique_temp_dir("pr-ops");
        let repo_dir = root.join("repo");
        assert!(std::fs::create_dir_all(&repo_dir).is_ok());
        assert!(git_service::run_git(&repo_dir, &["init"]).is_ok());
        assert!(
            git_service::run_git(&repo_dir, &["config", "user.email", "dev@example.com"]).is_ok()
        );
        assert!(git_service::run_git(&repo_dir, &["config", "user.name", "Dev User"]).is_ok());
        assert!(std::fs::write(repo_dir.join("README.md"), "base\n").is_ok());
        assert!(git_service::stage_paths(&repo_dir, &["README.md".to_string()]).is_ok());
        assert!(git_service::commit_create(&repo_dir, "base").is_ok());
        assert!(
            git_service::remote_add(
                &repo_dir,
                "origin",
                "https://github.com/branchforge/app.git"
            )
            .is_ok()
        );

        let mut runner = ConsoleRunner::new(test_config(&root));
        assert!(
            runner
                .execute(ConsoleCommand::Open {
                    path: repo_dir.to_string_lossy().to_string(),
                })
                .is_ok()
        );
        let created = runner
            .execute(ConsoleCommand::Run {
                target: "pr.create_url".to_string(),
                args: vec![
                    "main".to_string(),
                    "feature/collab".to_string(),
                    "Collaboration".to_string(),
                    "Layer".to_string(),
                ],
                confirmed: false,
            })
            .expect("create pr url");
        let message = created.message.unwrap_or_default();
        assert!(
            message.contains("https://github.com/branchforge/app/compare/main...feature/collab")
        );
        assert_eq!(
            runner
                .store
                .snapshot()
                .pull_requests
                .detected_provider
                .as_ref()
                .map(|provider| provider.provider.clone()),
            Some(state_store::ProviderKind::GitHub)
        );

        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn pr_list_uses_provider_api_and_stored_token() {
        let root = unique_temp_dir("pr-list-api");
        let repo_dir = root.join("repo");
        assert!(std::fs::create_dir_all(&repo_dir).is_ok());
        assert!(git_service::run_git(&repo_dir, &["init"]).is_ok());
        assert!(
            git_service::run_git(&repo_dir, &["config", "user.email", "dev@example.com"]).is_ok()
        );
        assert!(git_service::run_git(&repo_dir, &["config", "user.name", "Dev User"]).is_ok());
        assert!(std::fs::write(repo_dir.join("README.md"), "base\n").is_ok());
        assert!(git_service::stage_paths(&repo_dir, &["README.md".to_string()]).is_ok());
        assert!(git_service::commit_create(&repo_dir, "base").is_ok());
        assert!(
            git_service::remote_add(
                &repo_dir,
                "origin",
                "https://github.com/branchforge/app.git"
            )
            .is_ok()
        );

        let pulls = serde_json::json!([{
            "number": 12,
            "title": "Live provider API",
            "state": "open",
            "draft": false,
            "html_url": "https://github.com/branchforge/app/pull/12",
            "user": {"login": "octo"},
            "head": {"ref": "feature/provider-api", "sha": "abc123"},
            "base": {"ref": "main"}
        }])
        .to_string();
        let status = serde_json::json!({
            "state": "success",
            "statuses": [{"context": "ci"}]
        })
        .to_string();
        let (api_base, handle) = spawn_json_http_server(vec![pulls, status]);
        let mut config = test_config(&root);
        config.github_api_base = Some(api_base);
        let mut runner = ConsoleRunner::new(config);

        assert!(
            runner
                .run_target("repo.open", &[repo_dir.display().to_string()], false)
                .is_ok()
        );
        assert!(
            runner
                .run_target(
                    "auth.login",
                    &[
                        "github.com".to_string(),
                        "octo".to_string(),
                        "secret-token".to_string(),
                    ],
                    false,
                )
                .is_ok()
        );
        assert!(runner.run_target("pr.list", &[], false).is_ok());
        let snapshot = runner.store.snapshot();
        assert_eq!(snapshot.pull_requests.pull_requests.len(), 1);
        assert_eq!(snapshot.pull_requests.pull_requests[0].number, 12);
        assert_eq!(
            snapshot.pull_requests.pull_requests[0].checks[0].status,
            CheckStatus::Success
        );
        assert!(handle.join().is_ok());

        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn auth_ops_store_tokens_outside_state_and_seed_git_helper() {
        let root = unique_temp_dir("auth-ops");
        let repo_dir = init_repo("auth-ops-repo");
        let helper_file = repo_dir.join("credentials.txt");
        let helper = format!("store --file={}", helper_file.display());
        assert!(git_service::run_git(&repo_dir, &["config", "credential.helper", &helper]).is_ok());

        let mut runner = ConsoleRunner::new(test_config(&root));
        assert!(
            runner
                .run_target("repo.open", &[repo_dir.display().to_string()], false)
                .is_ok()
        );
        assert!(
            runner
                .run_target(
                    "auth.login",
                    &[
                        "github.com".to_string(),
                        "octo".to_string(),
                        "secret-token".to_string(),
                    ],
                    false,
                )
                .is_ok()
        );

        let snapshot = runner.store.snapshot();
        assert_eq!(snapshot.remotes.auth.accounts.len(), 1);
        assert_eq!(snapshot.remotes.auth.accounts[0].host, "github.com");
        assert!(snapshot.remotes.auth.accounts[0].token_present);
        let metadata = std::fs::read_to_string(root.join("auth/accounts.json")).unwrap_or_default();
        assert!(metadata.contains("github.com"));
        assert!(!metadata.contains("secret-token"));
        let helper_contents = std::fs::read_to_string(helper_file).unwrap_or_default();
        assert!(helper_contents.contains("secret-token"));

        assert!(
            runner
                .run_target(
                    "auth.logout",
                    &["github.com".to_string(), "octo".to_string()],
                    true,
                )
                .is_ok()
        );
        assert!(runner.store.snapshot().remotes.auth.accounts.is_empty());

        let _ = std::fs::remove_dir_all(&root);
        let _ = std::fs::remove_dir_all(&repo_dir);
    }

    #[test]
    fn branch_stack_ops_create_detect_and_journal_restack() {
        let root = unique_temp_dir("stack-ops");
        let repo_dir = root.join("repo");
        assert!(std::fs::create_dir_all(&repo_dir).is_ok());
        assert!(git_service::run_git(&repo_dir, &["init"]).is_ok());
        assert!(
            git_service::run_git(&repo_dir, &["config", "user.email", "dev@example.com"]).is_ok()
        );
        assert!(git_service::run_git(&repo_dir, &["config", "user.name", "Dev User"]).is_ok());
        assert!(std::fs::write(repo_dir.join("README.md"), "base\n").is_ok());
        assert!(git_service::stage_paths(&repo_dir, &["README.md".to_string()]).is_ok());
        assert!(git_service::commit_create(&repo_dir, "base").is_ok());
        let base = git_service::run_git(&repo_dir, &["branch", "--show-current"])
            .ok()
            .and_then(|out| String::from_utf8(out.stdout).ok())
            .map(|text| text.trim().to_string())
            .filter(|text| !text.is_empty())
            .unwrap_or_else(|| "master".to_string());
        assert!(git_service::create_branch(&repo_dir, "feature/stack").is_ok());
        assert!(git_service::checkout_branch(&repo_dir, "feature/stack").is_ok());
        assert!(std::fs::write(repo_dir.join("stack.txt"), "stack\n").is_ok());
        assert!(git_service::stage_paths(&repo_dir, &["stack.txt".to_string()]).is_ok());
        assert!(git_service::commit_create(&repo_dir, "stack change").is_ok());
        assert!(git_service::checkout_branch(&repo_dir, &base).is_ok());

        let mut runner = ConsoleRunner::new(test_config(&root));
        assert!(
            runner
                .execute(ConsoleCommand::Open {
                    path: repo_dir.to_string_lossy().to_string(),
                })
                .is_ok()
        );
        assert!(
            runner
                .execute(ConsoleCommand::Run {
                    target: "stack.create".to_string(),
                    args: vec![
                        "Demo".to_string(),
                        base.clone(),
                        "feature/stack".to_string()
                    ],
                    confirmed: false,
                })
                .is_ok()
        );
        assert_eq!(
            runner.store.snapshot().branch_stacks.stacks[0].entries[0].ahead,
            1
        );
        assert!(
            runner
                .execute(ConsoleCommand::Run {
                    target: "stack.restack".to_string(),
                    args: vec!["stack-demo".to_string()],
                    confirmed: true,
                })
                .is_ok()
        );
        assert!(
            runner
                .store
                .snapshot()
                .journal
                .entries
                .iter()
                .any(|entry| entry.op == "stack.restack_branch")
        );

        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn one_shot_console_command_runs_release_notes_runtime_flow() {
        let root = unique_temp_dir("one-shot-release-notes");
        assert!(std::fs::create_dir_all(&root).is_ok());
        let out_file = root.join("release_notes.md");

        let output = run_console_command(
            &format!("run release.notes {} stable", out_file.to_string_lossy()),
            test_config(&root),
            false,
        )
        .expect("one-shot command");

        assert!(output.stderr.is_empty());
        assert!(output.stdout.contains("release notes generated at"));
        let rendered = std::fs::read_to_string(&out_file).unwrap_or_default();
        assert!(rendered.contains("Branchforge"));
        assert!(rendered.contains("Channel: stable"));

        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn dispatcher_opens_repo_and_switches_panel() {
        let repo_dir = init_repo("dispatch-open");
        assert!(std::fs::write(repo_dir.join("README.md"), "hello\n").is_ok());

        let mut runner = ConsoleRunner::new(test_config(&repo_dir));
        let open = runner.execute(ConsoleCommand::Open {
            path: repo_dir.to_string_lossy().to_string(),
        });
        assert!(open.is_ok());
        let expected_root = std::fs::canonicalize(&repo_dir).unwrap_or(repo_dir.clone());
        assert_eq!(
            runner
                .store
                .snapshot()
                .repo
                .as_ref()
                .map(|repo| std::path::PathBuf::from(repo.root.clone())),
            Some(expected_root)
        );

        let panel = runner.execute(ConsoleCommand::Panel {
            panel: PanelKind::History,
        });
        assert!(panel.is_ok());
        assert_eq!(
            runner.store.snapshot().active_view.as_deref(),
            Some("history.panel")
        );

        let _ = std::fs::remove_dir_all(&repo_dir);
    }

    #[test]
    fn logs_panel_is_available_without_repo() {
        let root = unique_temp_dir("logs-panel");
        assert!(std::fs::create_dir_all(&root).is_ok());
        let mut runner = ConsoleRunner::new(test_config(&root));

        let panel = runner.execute(ConsoleCommand::Panel {
            panel: PanelKind::Logs,
        });
        assert!(panel.is_ok());
        assert_eq!(
            runner.store.snapshot().active_view.as_deref(),
            Some("logs.panel")
        );

        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn repo_open_preserves_plugin_selection_in_runner_state() {
        let repo_dir = init_repo("open-preserves-plugin");
        let mut runner = ConsoleRunner::new(test_config(&repo_dir));
        runner
            .store
            .update_selected_plugin(Some("status".to_string()));

        let open = runner.execute(ConsoleCommand::Open {
            path: repo_dir.to_string_lossy().to_string(),
        });
        assert!(open.is_ok());
        assert_eq!(
            runner
                .store
                .snapshot()
                .selection
                .selected_plugin_id
                .as_deref(),
            Some("status")
        );

        let _ = std::fs::remove_dir_all(&repo_dir);
    }

    #[test]
    fn dispatcher_requires_confirmation_for_high_risk_action() {
        let repo_dir = init_repo("confirm");
        let mut runner = ConsoleRunner::new(test_config(&repo_dir));
        let open = runner.execute(ConsoleCommand::Open {
            path: repo_dir.to_string_lossy().to_string(),
        });
        assert!(open.is_ok());

        let error = runner
            .execute(ConsoleCommand::Run {
                target: "reset.hard".to_string(),
                args: vec!["HEAD".to_string()],
                confirmed: false,
            })
            .expect_err("confirmation required");
        assert_eq!(error.title, "Confirmation required");

        let _ = std::fs::remove_dir_all(&repo_dir);
    }

    #[test]
    fn dispatcher_select_file_loads_diff() {
        let repo_dir = init_repo("select-file");
        assert!(std::fs::write(repo_dir.join("tracked.txt"), "line\n").is_ok());
        let mut runner = ConsoleRunner::new(test_config(&repo_dir));
        assert!(
            runner
                .execute(ConsoleCommand::Open {
                    path: repo_dir.to_string_lossy().to_string(),
                })
                .is_ok()
        );

        assert!(
            runner
                .execute(ConsoleCommand::Select {
                    target: SelectionTarget::File,
                    value: "tracked.txt".to_string(),
                })
                .is_ok()
        );
        assert!(matches!(
            runner.store.snapshot().diff.source,
            Some(state_store::DiffSource::Worktree { .. })
        ));

        let _ = std::fs::remove_dir_all(&repo_dir);
    }

    #[test]
    fn dispatcher_uses_selection_defaults_for_commit_branch_and_compare_actions() {
        let repo_dir = init_repo("selection-defaults");
        assert!(std::fs::write(repo_dir.join("README.md"), "base\n").is_ok());
        assert!(git_service::stage_paths(&repo_dir, &["README.md".to_string()]).is_ok());
        assert!(git_service::commit_create(&repo_dir, "base").is_ok());

        assert!(git_service::create_branch(&repo_dir, "feature/demo").is_ok());

        let commits = git_service::commit_log_page(&repo_dir, 0, 1).expect("commits");
        let selected_oid = commits[0].oid.clone();

        let mut runner = ConsoleRunner::new(test_config(&repo_dir));
        assert!(
            runner
                .execute(ConsoleCommand::Open {
                    path: repo_dir.to_string_lossy().to_string(),
                })
                .is_ok()
        );

        runner
            .store
            .update_selected_commit(Some(selected_oid.clone()));
        assert!(
            runner
                .execute(ConsoleCommand::Run {
                    target: "history.select_commit".to_string(),
                    args: Vec::new(),
                    confirmed: false,
                })
                .is_ok()
        );
        assert_eq!(
            runner
                .store
                .snapshot()
                .selection
                .selected_commit_oid
                .as_deref(),
            Some(selected_oid.as_str())
        );
        assert_eq!(
            runner.store.snapshot().active_view.as_deref(),
            Some("history.panel")
        );

        runner
            .store
            .update_selected_branch(Some("feature/demo".to_string()));
        assert!(
            runner
                .execute(ConsoleCommand::Run {
                    target: "compare.refs".to_string(),
                    args: Vec::new(),
                    confirmed: false,
                })
                .is_ok()
        );
        assert_eq!(
            runner.store.snapshot().compare.head_ref.as_deref(),
            Some("feature/demo")
        );
        assert_eq!(
            runner.store.snapshot().active_view.as_deref(),
            Some("compare.panel")
        );

        let _ = std::fs::remove_dir_all(&repo_dir);
    }

    #[test]
    fn diagnostics_journal_summary_populates_diff_summary() {
        let repo_dir = init_repo("journal-summary");
        let mut runner = ConsoleRunner::new(test_config(&repo_dir));
        assert!(
            runner
                .execute(ConsoleCommand::Open {
                    path: repo_dir.to_string_lossy().to_string(),
                })
                .is_ok()
        );

        assert!(
            runner
                .execute(ConsoleCommand::Run {
                    target: "diagnostics.journal_summary".to_string(),
                    args: Vec::new(),
                    confirmed: false,
                })
                .is_ok()
        );

        let diff = runner
            .store
            .snapshot()
            .diff
            .content
            .clone()
            .unwrap_or_default();
        assert!(diff.contains("Journal Summary"));
        assert!(diff.contains("entries:"));
        assert_eq!(
            runner.store.snapshot().active_view.as_deref(),
            Some("logs.panel")
        );

        let _ = std::fs::remove_dir_all(&repo_dir);
    }

    #[test]
    fn plugin_install_and_list_syncs_inventory_into_diagnostics_state() {
        let root = unique_temp_dir("plugin-inventory");
        assert!(std::fs::create_dir_all(&root).is_ok());
        let package_dir = create_plugin_package(&root, "sample_status");
        let mut runner = ConsoleRunner::new(test_config(&root));

        assert!(
            runner
                .execute(ConsoleCommand::Plugin {
                    op: PluginOp::Install {
                        package_dir: package_dir.display().to_string(),
                    },
                    confirmed: false,
                })
                .is_ok()
        );
        assert_eq!(runner.store.snapshot().installed_plugins.len(), 1);
        assert_eq!(
            runner.store.snapshot().installed_plugins[0].plugin_id,
            "sample_status"
        );
        assert_eq!(runner.store.snapshot().plugin_security.len(), 1);
        assert_eq!(
            runner.store.snapshot().plugin_security[0].trust_level,
            PluginTrustLevel::UnsignedLocal
        );
        assert!(
            runner.store.snapshot().plugin_security[0]
                .warnings
                .iter()
                .any(|warning| warning.contains("signature"))
        );
        assert_eq!(
            runner.store.snapshot().active_view.as_deref(),
            Some("diagnostics.panel")
        );

        assert!(
            runner
                .execute(ConsoleCommand::Plugin {
                    op: PluginOp::List,
                    confirmed: false,
                })
                .is_ok()
        );
        let diagnostics = ui_shell::render_diagnostics_panel(&runner.store);
        assert!(diagnostics.contains("Installed plugins: 1"));
        assert!(diagnostics.contains("Selected plugin: sample_status"));
        assert!(diagnostics.contains("* sample_status v0.1.0 enabled"));
        assert_eq!(
            runner
                .store
                .snapshot()
                .selection
                .selected_plugin_id
                .as_deref(),
            Some("sample_status")
        );

        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn plugin_discover_and_install_registry_flow_updates_diagnostics_state() {
        let root = unique_temp_dir("plugin-registry-runner");
        assert!(std::fs::create_dir_all(&root).is_ok());
        let package_dir = create_plugin_package(&root, "sample_status");
        let registry_dir = create_plugin_registry(&root, "sample_status", &package_dir);
        let mut runner = ConsoleRunner::new(test_config(&root));

        assert!(
            runner
                .execute(ConsoleCommand::Plugin {
                    op: PluginOp::Discover {
                        registry_path: Some(registry_dir.display().to_string()),
                    },
                    confirmed: false,
                })
                .is_ok()
        );
        let discover_diff = runner
            .store
            .snapshot()
            .diff
            .content
            .clone()
            .unwrap_or_default();
        assert!(discover_diff.contains("registry:"));
        assert!(discover_diff.contains("sample_status v0.1.0"));

        assert!(
            runner
                .execute(ConsoleCommand::Plugin {
                    op: PluginOp::InstallRegistry {
                        plugin_id: "sample_status".to_string(),
                        registry_path: Some(registry_dir.display().to_string()),
                    },
                    confirmed: false,
                })
                .is_ok()
        );
        assert_eq!(runner.store.snapshot().installed_plugins.len(), 1);
        assert_eq!(
            runner
                .store
                .snapshot()
                .selection
                .selected_plugin_id
                .as_deref(),
            Some("sample_status")
        );

        let mut manifest: serde_json::Value = serde_json::from_str(
            &std::fs::read_to_string(package_dir.join("plugin.json")).unwrap_or_default(),
        )
        .unwrap_or_else(|_| serde_json::json!({}));
        manifest["version"] = serde_json::Value::String("0.2.0".to_string());
        assert!(std::fs::write(package_dir.join("plugin.json"), manifest.to_string()).is_ok());
        assert!(
            runner
                .execute(ConsoleCommand::Plugin {
                    op: PluginOp::Marketplace {
                        registry_path: Some(registry_dir.display().to_string()),
                    },
                    confirmed: false,
                })
                .is_ok()
        );
        let marketplace_diff = runner
            .store
            .snapshot()
            .diff
            .content
            .clone()
            .unwrap_or_default();
        assert!(marketplace_diff.contains("update=true"));
        assert!(runner.store.snapshot().plugin_security[0].update_available);
        assert!(
            runner
                .execute(ConsoleCommand::Plugin {
                    op: PluginOp::Update {
                        plugin_id: "sample_status".to_string(),
                        registry_path: Some(registry_dir.display().to_string()),
                    },
                    confirmed: true,
                })
                .is_ok()
        );
        assert_eq!(
            runner.store.snapshot().installed_plugins[0].version,
            "0.2.0"
        );

        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn diagnostics_lfs_ops_work_through_console_runtime() {
        let Some((repo_dir, payload)) = init_lfs_runtime_repo("lfs-console") else {
            return;
        };
        let mut runner = ConsoleRunner::new(test_config(&repo_dir));

        assert!(
            runner
                .execute(ConsoleCommand::Open {
                    path: repo_dir.to_string_lossy().to_string(),
                })
                .is_ok()
        );

        assert!(
            runner
                .execute(ConsoleCommand::Run {
                    target: "diagnostics.lfs_status".to_string(),
                    args: Vec::new(),
                    confirmed: false,
                })
                .is_ok()
        );
        assert_eq!(
            runner.store.snapshot().active_view.as_deref(),
            Some("diagnostics.panel")
        );

        let pointer_before =
            std::fs::read_to_string(repo_dir.join("payload.bin")).unwrap_or_default();
        assert!(pointer_before.contains("git-lfs.github.com/spec/v1"));

        assert!(
            runner
                .execute(ConsoleCommand::Run {
                    target: "diagnostics.lfs_fetch".to_string(),
                    args: Vec::new(),
                    confirmed: false,
                })
                .is_ok()
        );
        let pointer_after_fetch =
            std::fs::read_to_string(repo_dir.join("payload.bin")).unwrap_or_default();
        assert!(pointer_after_fetch.contains("git-lfs.github.com/spec/v1"));

        assert!(
            runner
                .execute(ConsoleCommand::Run {
                    target: "diagnostics.lfs_pull".to_string(),
                    args: Vec::new(),
                    confirmed: false,
                })
                .is_ok()
        );
        let content_after_pull =
            std::fs::read_to_string(repo_dir.join("payload.bin")).unwrap_or_default();
        assert_eq!(content_after_pull, payload);

        let _ =
            std::fs::remove_dir_all(repo_dir.parent().map(Path::to_path_buf).unwrap_or(repo_dir));
    }

    #[test]
    fn plugin_selection_enables_default_disable_and_remove_confirmation() {
        let root = unique_temp_dir("plugin-selection");
        assert!(std::fs::create_dir_all(&root).is_ok());
        let package_dir = create_plugin_package(&root, "sample_status");
        let mut runner = ConsoleRunner::new(test_config(&root));

        assert!(
            runner
                .execute(ConsoleCommand::Plugin {
                    op: PluginOp::Install {
                        package_dir: package_dir.display().to_string(),
                    },
                    confirmed: false,
                })
                .is_ok()
        );
        assert!(
            runner
                .execute(ConsoleCommand::Select {
                    target: SelectionTarget::Plugin,
                    value: "sample_status".to_string(),
                })
                .is_ok()
        );
        assert_eq!(
            runner
                .store
                .snapshot()
                .selection
                .selected_plugin_id
                .as_deref(),
            Some("sample_status")
        );

        assert!(
            runner
                .execute(ConsoleCommand::Plugin {
                    op: PluginOp::Disable { plugin_id: None },
                    confirmed: false,
                })
                .is_ok()
        );
        assert!(!runner.store.snapshot().installed_plugins[0].enabled);

        let error = runner
            .execute(ConsoleCommand::Plugin {
                op: PluginOp::Remove { plugin_id: None },
                confirmed: false,
            })
            .expect_err("confirmation required");
        assert_eq!(error.title, "Confirmation required");

        let actions = runner.render_actions();
        assert!(actions.contains("plugin.list"));
        assert!(actions.contains("plugin.remove"));

        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn sample_external_plugin_registers_dynamic_action_and_runs() {
        let root = unique_temp_dir("sample-external-runtime");
        assert!(std::fs::create_dir_all(&root).is_ok());
        let package_dir = build_sample_external_plugin();
        let mut runner = ConsoleRunner::new(test_config(&root));

        assert!(
            runner
                .execute(ConsoleCommand::Plugin {
                    op: PluginOp::Install {
                        package_dir: package_dir.display().to_string(),
                    },
                    confirmed: false,
                })
                .is_ok()
        );

        let actions = runner.render_actions();
        assert!(actions.contains("sample.ping"));

        let result = runner
            .execute(ConsoleCommand::Run {
                target: "sample.ping".to_string(),
                args: Vec::new(),
                confirmed: false,
            })
            .expect("run sample plugin action");
        assert_eq!(
            result.message.as_deref(),
            Some("sample_external handled request")
        );

        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn plugin_list_reconciles_stale_selection_after_external_removal() {
        let root = unique_temp_dir("plugin-reconcile");
        assert!(std::fs::create_dir_all(&root).is_ok());
        let package_dir = create_plugin_package(&root, "sample_status");
        let mut runner = ConsoleRunner::new(test_config(&root));

        assert!(
            runner
                .execute(ConsoleCommand::Plugin {
                    op: PluginOp::Install {
                        package_dir: package_dir.display().to_string(),
                    },
                    confirmed: false,
                })
                .is_ok()
        );
        assert!(
            runner
                .execute(ConsoleCommand::Select {
                    target: SelectionTarget::Plugin,
                    value: "sample_status".to_string(),
                })
                .is_ok()
        );
        assert_eq!(
            runner
                .store
                .snapshot()
                .selection
                .selected_plugin_id
                .as_deref(),
            Some("sample_status")
        );

        assert!(std::fs::remove_dir_all(root.join("plugins").join("sample_status")).is_ok());

        assert!(
            runner
                .execute(ConsoleCommand::Plugin {
                    op: PluginOp::List,
                    confirmed: false,
                })
                .is_ok()
        );
        assert!(
            runner
                .store
                .snapshot()
                .selection
                .selected_plugin_id
                .is_none()
        );

        let diagnostics = ui_shell::render_diagnostics_panel(&runner.store);
        assert!(diagnostics.contains("Selected plugin: <none>"));
        assert!(diagnostics.contains("Installed plugins: 0"));

        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn plugin_disable_reconciles_stale_selection_before_using_defaults() {
        let root = unique_temp_dir("plugin-reconcile-disable");
        assert!(std::fs::create_dir_all(&root).is_ok());
        let package_dir = create_plugin_package(&root, "sample_status");
        let mut runner = ConsoleRunner::new(test_config(&root));

        assert!(
            runner
                .execute(ConsoleCommand::Plugin {
                    op: PluginOp::Install {
                        package_dir: package_dir.display().to_string(),
                    },
                    confirmed: false,
                })
                .is_ok()
        );
        assert!(
            runner
                .execute(ConsoleCommand::Select {
                    target: SelectionTarget::Plugin,
                    value: "sample_status".to_string(),
                })
                .is_ok()
        );
        assert!(std::fs::remove_dir_all(root.join("plugins").join("sample_status")).is_ok());

        let error = runner
            .execute(ConsoleCommand::Plugin {
                op: PluginOp::Disable { plugin_id: None },
                confirmed: false,
            })
            .expect_err("stale selection should be cleared before disable");
        assert_eq!(error.title, "Invalid input");
        assert!(
            runner
                .store
                .snapshot()
                .selection
                .selected_plugin_id
                .is_none()
        );

        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn scripted_session_handles_open_actions_run_show_quit() {
        let root = unique_temp_dir("script");
        let repo_dir = root.join("repo");
        assert!(std::fs::create_dir_all(&repo_dir).is_ok());
        assert!(git_service::run_git(&repo_dir, &["init"]).is_ok());
        assert!(
            git_service::run_git(&repo_dir, &["config", "user.email", "dev@example.com"]).is_ok()
        );
        assert!(git_service::run_git(&repo_dir, &["config", "user.name", "Dev User"]).is_ok());
        assert!(std::fs::write(repo_dir.join("README.md"), "base\n").is_ok());
        assert!(git_service::stage_paths(&repo_dir, &["README.md".to_string()]).is_ok());
        assert!(git_service::commit_create(&repo_dir, "base").is_ok());

        let script = format!(
            "open {}\nactions\nrun diagnostics.repo_capabilities\nshow\nquit\n",
            repo_dir.display()
        );
        let output = run_scripted_console_session(&script, test_config(&root)).expect("session");

        assert!(output.stdout.contains("opened repository"));
        assert!(output.stdout.contains("Actions"));
        assert!(output.stdout.contains("diagnostics.repo_capabilities"));
        assert!(output.stdout.contains("[window]"));
        assert!(output.stdout.contains("Host version:"));
        assert!(output.stdout.contains("lfs_detected:"));
        assert!(output.stdout.contains("bye"));

        let _ = std::fs::remove_dir_all(&root);
    }
}
