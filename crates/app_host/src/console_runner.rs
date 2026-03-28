use std::io::{BufRead, Write};
use std::path::{Path, PathBuf};

use action_engine::{ActionRequest, validate_action};
use job_system::{JobExecutionResult, JobLock, JobRequest, execute_job_op};
use plugin_api::{ActionEffects, ActionSpec, ConfirmPolicy, DangerLevel};
use plugin_host::{
    PluginManagerError, branches_registration_payload, compare_registration_payload,
    diagnostics_registration_payload, discover_local_plugins, history_registration_payload,
    install_local_plugin, install_registry_plugin, list_installed_plugins, remove_local_plugin,
    repo_manager_registration_payload, set_plugin_enabled, status_registration_payload,
    tags_registration_payload,
};
use state_store::{DiffSource, DiffState, InstalledPluginRecord, StateStore};

use crate::errors::{ErrorCategory, UserFacingError, translate_job_error};
use crate::operations;
use crate::recent_repos::persist_recent_repo;
use crate::run_rebase_beta_smoke;

#[cfg(test)]
use std::io::Cursor;

#[derive(Debug, Clone)]
pub struct ConsoleRunnerConfig {
    pub cwd: PathBuf,
    pub plugins_root: PathBuf,
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
            auto_render: true,
        })
    }
}

impl Default for ConsoleRunnerConfig {
    fn default() -> Self {
        Self::from_current_env().unwrap_or_else(|_| Self {
            cwd: PathBuf::from("."),
            plugins_root: PathBuf::from("target/tmp/console-runner/plugins"),
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
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum PluginOp {
    List,
    Discover {
        registry_path: Option<String>,
    },
    Install {
        package_dir: String,
    },
    InstallRegistry {
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
    owner: &'static str,
    spec: ActionSpec,
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
        Self {
            config,
            store: StateStore::new(),
            repo_dir: None,
            actions: build_catalog_actions(),
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

    fn action_availability(&self, spec: &ActionSpec) -> (bool, Option<String>) {
        let repo_open = self.repo_dir.is_some();
        if matches!(spec.when.as_deref(), Some("repo.is_open")) && !repo_open {
            return (false, Some("repository is not open".to_string()));
        }

        if let Some(owner) = ui_shell::palette::action_owner_plugin(&spec.action_id)
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

        match spec.action_id.as_str() {
            "index.stage_selected" | "index.unstage_selected" | "file.discard" => {
                if self.store.snapshot().selection.selected_paths.is_empty() {
                    return (false, Some("no selected files".to_string()));
                }
            }
            "index.stage_hunk" | "index.stage_lines" | "file.discard_hunk"
            | "file.discard_lines" => {
                if self.store.snapshot().diff.hunks.is_empty()
                    || !matches!(
                        self.store.snapshot().diff.source,
                        Some(state_store::DiffSource::Worktree { .. })
                    )
                {
                    return (
                        false,
                        Some("load a worktree diff with hunks first".to_string()),
                    );
                }
            }
            "index.unstage_hunk" | "index.unstage_lines" => {
                if self.store.snapshot().diff.hunks.is_empty()
                    || !matches!(
                        self.store.snapshot().diff.source,
                        Some(state_store::DiffSource::Index { .. })
                    )
                {
                    return (
                        false,
                        Some("load an index diff with hunks first".to_string()),
                    );
                }
            }
            "commit.create" => {
                if self.store.snapshot().status.staged.is_empty() {
                    return (false, Some("no staged changes".to_string()));
                }
            }
            "history.load_more" => {
                if self.store.snapshot().history.next_cursor.is_none() {
                    return (false, Some("no next history page".to_string()));
                }
            }
            "history.select_commit" | "cherry_pick.commit" | "revert.commit" => {
                if self
                    .store
                    .snapshot()
                    .selection
                    .selected_commit_oid
                    .is_none()
                {
                    return (false, Some("no selected commit".to_string()));
                }
            }
            "history.file" | "blame.file" => {
                if self.store.snapshot().selection.selected_paths.is_empty() {
                    return (false, Some("no selected files".to_string()));
                }
            }
            "branch.checkout" | "branch.rename" | "branch.delete" => {
                if self.store.snapshot().selection.selected_branch.is_none() {
                    return (false, Some("no selected branch".to_string()));
                }
            }
            "rebase.execute" => {
                if self.store.snapshot().rebase.plan.is_none() {
                    return (false, Some("no rebase plan".to_string()));
                }
            }
            "rebase.plan.set_action" | "rebase.plan.move" | "rebase.plan.clear" => {
                if self.store.snapshot().rebase.plan.is_none() {
                    return (false, Some("no rebase plan".to_string()));
                }
            }
            "rebase.continue" | "rebase.skip" | "rebase.abort" => {
                if self.store.snapshot().rebase.session.is_none() {
                    return (false, Some("no active rebase session".to_string()));
                }
            }
            "conflict.focus" => {
                if self.store.snapshot().selection.selected_paths.is_empty() {
                    return (false, Some("no selected conflict files".to_string()));
                }
            }
            "conflict.resolve.ours" | "conflict.resolve.theirs" | "conflict.mark_resolved" => {
                if self.store.snapshot().selection.selected_paths.is_empty() {
                    return (false, Some("no selected conflict files".to_string()));
                }
            }
            "conflict.continue" | "conflict.abort" => {
                if self
                    .store
                    .snapshot()
                    .repo
                    .as_ref()
                    .and_then(|repo| repo.conflict_state.as_ref())
                    .is_none()
                {
                    return (false, Some("no active conflict session".to_string()));
                }
            }
            "plugin.enable" | "plugin.disable" | "plugin.remove" => {
                if self.store.snapshot().selection.selected_plugin_id.is_none() {
                    return (
                        false,
                        Some("no selected plugin (or pass id explicitly)".to_string()),
                    );
                }
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
                    self.show_journal_summary();
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
            "plugin.list" => self.run_plugin_op(PluginOp::List, confirmed),
            "plugin.discover" => {
                let registry_path = args.first().cloned();
                self.run_plugin_op(PluginOp::Discover { registry_path }, confirmed)
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

    fn run_plugin_op(&mut self, op: PluginOp, confirmed: bool) -> Result<String, UserFacingError> {
        let action_id = match &op {
            PluginOp::List => "plugin.list",
            PluginOp::Discover { .. } => "plugin.discover",
            PluginOp::Install { .. } => "plugin.install",
            PluginOp::InstallRegistry { .. } => "plugin.install_registry",
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
        if let Some(selected_plugin_id) = self.store.snapshot().selection.selected_plugin_id.clone()
            && !installed
                .iter()
                .any(|plugin| plugin.manifest.plugin_id == selected_plugin_id)
        {
            self.store.update_selected_plugin(None);
        }
        self.store
            .update_installed_plugins(map_installed_plugins(&installed));
        Ok(installed)
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
            "diagnostics.repo_capabilities"
            | "diagnostics.journal_summary"
            | "diagnostics.lfs_status"
            | "diagnostics.lfs_fetch"
            | "diagnostics.lfs_pull" => Some(PanelKind::Diagnostics.view_id()),
            _ => None,
        };

        if let Some(view) = view {
            self.store.set_active_view(Some(view.to_string()));
        }
    }

    fn show_journal_summary(&mut self) {
        self.store
            .set_active_view(Some(PanelKind::Diagnostics.view_id().to_string()));
        self.store.update_diff(render_text_diff(
            "diagnostics:journal_summary",
            render_journal_summary(&self.store),
        ));
        self.last_replayable = Some(ReplayableRun::Run {
            target: "diagnostics.journal_summary".to_string(),
            args: Vec::new(),
        });
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
}

fn build_catalog_actions() -> Vec<CatalogAction> {
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

fn push_actions(actions: &mut Vec<CatalogAction>, owner: &'static str, specs: Vec<ActionSpec>) {
    actions.extend(specs.into_iter().map(|spec| CatalogAction { owner, spec }));
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
            "plugin.discover",
            "Discover Registry Plugins",
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
                "usage: plugin <list|discover|install|install-registry|enable|disable|remove> ..."
                    .to_string()
            })?;
            let op = match subcommand {
                "list" => PluginOp::List,
                "discover" => PluginOp::Discover {
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
                    return Err("plugin must be one of: list, discover, install, install-registry, enable, disable, remove".to_string());
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
        "plugin.install <package_dir>",
        "plugin.install_registry <plugin_id> [registry_path]",
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
        "diagnostics.lfs_fetch" | "diagnostics.lfs_pull" => JobLock::Network,
        "commit.create" | "commit.amend" | "worktree.create" | "worktree.remove" | "stash.drop"
        | "merge.execute" | "merge.abort" | "cherry_pick.commit" | "cherry_pick.abort"
        | "revert.commit" | "rebase.plan.create" | "rebase.execute" | "rebase.continue"
        | "rebase.skip" | "rebase.abort" | "branch.checkout" | "branch.create"
        | "branch.rename" | "branch.delete" | "tag.create" | "tag.delete" | "tag.checkout"
        | "file.discard" | "conflict.continue" | "conflict.abort" => JobLock::RefsWrite,
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
            | "plugin.list"
            | "plugin.discover"
            | "plugin.install"
            | "plugin.install_registry"
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
            "{} v{} channel={} {} perms={}",
            plugin.manifest.plugin_id,
            plugin.manifest.version,
            plugin.channel.as_deref().unwrap_or("stable"),
            package_label,
            plugin.manifest.permissions.join(", ")
        ));
        if let Some(summary) = plugin.summary.as_deref() {
            lines.push(format!("  summary: {summary}"));
        }
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
            lines.push(format!(
                "#{} {} {} duration={}{}",
                entry.id, status, entry.op, duration, suffix
            ));
        }
    }

    lines.join("\n")
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

    fn test_config(root: &Path) -> ConsoleRunnerConfig {
        ConsoleRunnerConfig {
            cwd: root.to_path_buf(),
            plugins_root: root.join("plugins"),
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
        assert_eq!(
            runner
                .store
                .snapshot()
                .repo
                .as_ref()
                .map(|repo| repo.root.clone()),
            Some(repo_dir.to_string_lossy().to_string())
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
            Some("diagnostics.panel")
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
