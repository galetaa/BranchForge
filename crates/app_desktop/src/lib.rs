pub mod runtime_adapter;
pub mod ui_state;

use std::time::Duration;

use app_host::HostActionCatalogItem;
use eframe::egui::{self, Color32, RichText};
use graph_model::{
    GraphCommit, GraphInputCommit, GraphRef, GraphRefKind, GraphRefLabel, build_graph,
};
use plugin_api::{ConfirmPolicy, DangerLevel};
use runtime_adapter::{DesktopRuntimeAdapter, DesktopRuntimeError, RuntimeAdapterState};
use state_store::{DiffSource, JournalStatus, OperationJournalEntry, StoreSnapshot};
use ui_state::{ConfirmationDialog, DesktopUiState, PanelId};

const MAX_RENDERED_DIFF_LINES: usize = 900;

pub fn run() -> eframe::Result<()> {
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_inner_size([1280.0, 820.0])
            .with_min_inner_size([960.0, 640.0]),
        ..Default::default()
    };

    eframe::run_native(
        "BranchForge Desktop",
        options,
        Box::new(|cc| Ok(Box::new(BranchForgeDesktopApp::new(cc)))),
    )
}

pub fn smoke_launch() -> Result<String, String> {
    let runtime = DesktopRuntimeAdapter::from_current_env()?;
    let state = runtime.state();
    if state.action_catalog.is_empty() {
        return Err("desktop runtime action catalog is empty".to_string());
    }
    Ok(format!(
        "desktop smoke launch ok: actions={}",
        state.action_catalog.len()
    ))
}

pub struct BranchForgeDesktopApp {
    runtime: DesktopRuntimeAdapter,
    ui_state: DesktopUiState,
    repo_path_input: String,
    commit_message: String,
    branch_name_input: String,
    compare_base_input: String,
    compare_head_input: String,
    ui_error: Option<String>,
}

impl BranchForgeDesktopApp {
    pub fn new(cc: &eframe::CreationContext<'_>) -> Self {
        cc.egui_ctx.set_visuals(egui::Visuals::dark());
        let (runtime, ui_error) = match DesktopRuntimeAdapter::from_current_env() {
            Ok(runtime) => (runtime, None),
            Err(error) => (
                DesktopRuntimeAdapter::new(app_host::ConsoleRunnerConfig::default()),
                Some(format!("Desktop bootstrap used default config: {error}")),
            ),
        };

        Self {
            runtime,
            ui_state: DesktopUiState::default(),
            repo_path_input: String::new(),
            commit_message: String::new(),
            branch_name_input: String::new(),
            compare_base_input: String::new(),
            compare_head_input: String::new(),
            ui_error,
        }
    }

    fn handle_shortcuts(&mut self, ctx: &egui::Context) {
        let open_palette = ctx.input(|input| {
            (input.key_pressed(egui::Key::K) && input.modifiers.command)
                || (input.key_pressed(egui::Key::P)
                    && input.modifiers.command
                    && input.modifiers.shift)
        });
        if open_palette {
            self.ui_state.command_palette.open = true;
        }

        let open_repo = ctx.input(|input| {
            input.key_pressed(egui::Key::O) && input.modifiers.command && !input.modifiers.shift
        });
        if open_repo {
            self.open_repo_dialog();
        }

        let refresh = ctx.input(|input| input.key_pressed(egui::Key::R) && input.modifiers.command);
        if refresh {
            let result = self.runtime.refresh();
            self.record_submit(result);
        }

        let toggle_left =
            ctx.input(|input| input.key_pressed(egui::Key::B) && input.modifiers.command);
        if toggle_left {
            self.ui_state.layout.left_sidebar_open = !self.ui_state.layout.left_sidebar_open;
        }
    }

    fn open_repo_from_input(&mut self) {
        let path = self.repo_path_input.trim().to_string();
        if path.is_empty() {
            self.ui_error = Some("Repository path is empty.".to_string());
            return;
        }
        let result = self.runtime.open_repo(path);
        self.record_submit(result);
    }

    fn open_repo_dialog(&mut self) {
        let dialog = if self.repo_path_input.trim().is_empty() {
            rfd::FileDialog::new()
        } else {
            rfd::FileDialog::new().set_directory(self.repo_path_input.trim())
        };
        if let Some(path) = dialog.pick_folder() {
            self.repo_path_input = path.to_string_lossy().to_string();
            self.open_repo_from_input();
        }
    }

    fn activate_panel(&mut self, panel: PanelId) {
        self.ui_state.active_panel = panel;
        self.ui_state.selected_sidebar_item = panel;
        if let Some(host_panel) = panel.host_panel() {
            let result = self.runtime.switch_panel(host_panel);
            self.record_submit(result);
        }
    }

    fn record_submit(&mut self, result: Result<(), DesktopRuntimeError>) {
        if let Err(error) = result {
            self.ui_error = Some(error.to_string());
        }
    }

    fn execute_or_confirm(
        &mut self,
        item: &HostActionCatalogItem,
        args: Vec<String>,
        title: impl Into<String>,
    ) {
        if action_requires_confirmation(item) {
            self.ui_state.pending_confirmation = Some(ConfirmationDialog {
                action_id: item.action_id.clone(),
                args,
                title: title.into(),
                message: format!(
                    "{} requires confirmation because its danger level is {}.",
                    item.action_id,
                    format_danger(&item.danger)
                ),
            });
            return;
        }

        let result = self.runtime.execute_action(&item.action_id, &args, false);
        self.record_submit(result);
    }

    fn execute_action_direct(&mut self, action_id: &str, args: Vec<String>, confirmed: bool) {
        let result = self.runtime.execute_action(action_id, &args, confirmed);
        self.record_submit(result);
    }

    fn selected_file<'a>(&self, snapshot: &'a StoreSnapshot) -> Option<&'a str> {
        snapshot
            .selection
            .selected_paths
            .first()
            .map(String::as_str)
    }

    fn render_top_bar(&mut self, root_ui: &mut egui::Ui, state: &RuntimeAdapterState) {
        let ctx = root_ui.ctx().clone();
        egui::Panel::top("branchforge.top_bar").show_inside(root_ui, |ui| {
            ui.horizontal(|ui| {
                if ui.button("Sidebar").clicked() {
                    self.ui_state.layout.left_sidebar_open =
                        !self.ui_state.layout.left_sidebar_open;
                }
                if ui.button("Inspector").clicked() {
                    self.ui_state.layout.right_inspector_open =
                        !self.ui_state.layout.right_inspector_open;
                }
                ui.separator();
                ui.label(RichText::new("BranchForge").strong());
                ui.separator();
                let repo_label = state
                    .snapshot
                    .repo
                    .as_ref()
                    .map(|repo| repo.root.as_str())
                    .unwrap_or("<no repo>");
                ui.label(repo_label);
                if let Some(head) = state
                    .snapshot
                    .repo
                    .as_ref()
                    .and_then(|repo| repo.head.as_deref())
                {
                    ui.label(RichText::new(head).color(Color32::from_rgb(125, 211, 252)));
                }
                ui.separator();
                let path_response = ui.add_enabled(
                    !state.busy,
                    egui::TextEdit::singleline(&mut self.repo_path_input)
                        .desired_width(280.0)
                        .hint_text("Repository path"),
                );
                if path_response.lost_focus()
                    && ui.input(|input| input.key_pressed(egui::Key::Enter))
                {
                    self.open_repo_from_input();
                }
                if ui
                    .add_enabled(!state.busy, egui::Button::new("Open"))
                    .clicked()
                {
                    self.open_repo_from_input();
                }
                if ui
                    .add_enabled(!state.busy, egui::Button::new("Browse"))
                    .clicked()
                {
                    self.open_repo_dialog();
                }
                if ui
                    .add_enabled(!state.busy, egui::Button::new("Refresh"))
                    .clicked()
                {
                    let result = self.runtime.refresh();
                    self.record_submit(result);
                }
                if ui.button("Palette").clicked() {
                    self.ui_state.command_palette.open = true;
                }
                ui.checkbox(&mut self.ui_state.layout.advanced_mode, "Advanced");
                let mut dark = self.ui_state.layout.dark_mode;
                if ui.checkbox(&mut dark, "Dark").changed() {
                    self.ui_state.layout.dark_mode = dark;
                    if dark {
                        ctx.set_visuals(egui::Visuals::dark());
                    } else {
                        ctx.set_visuals(egui::Visuals::light());
                    }
                }
                if state.busy {
                    ui.spinner();
                }
            });
        });
    }

    fn render_sidebar(&mut self, root_ui: &mut egui::Ui) {
        if !self.ui_state.layout.left_sidebar_open {
            return;
        }

        egui::Panel::left("branchforge.sidebar")
            .resizable(true)
            .default_size(190.0)
            .size_range(150.0..=280.0)
            .show_inside(root_ui, |ui| {
                ui.vertical(|ui| {
                    ui.label(RichText::new("Panels").strong());
                    ui.separator();
                    for panel in PanelId::ALL {
                        if ui
                            .selectable_label(self.ui_state.active_panel == panel, panel.label())
                            .clicked()
                        {
                            self.activate_panel(panel);
                        }
                    }
                });
            });
    }

    fn render_inspector(&mut self, root_ui: &mut egui::Ui, state: &RuntimeAdapterState) {
        if !self.ui_state.layout.right_inspector_open {
            return;
        }

        egui::Panel::right("branchforge.inspector")
            .resizable(true)
            .default_size(260.0)
            .size_range(220.0..=380.0)
            .show_inside(root_ui, |ui| {
                egui::ScrollArea::vertical().show(ui, |ui| {
                    ui.label(RichText::new("Inspector").strong());
                    ui.separator();
                    ui.label(format!("Panel: {}", self.ui_state.active_panel.label()));
                    if let Some(file) = self.selected_file(&state.snapshot) {
                        ui.label(format!("File: {file}"));
                    }
                    if let Some(commit) = state.snapshot.selection.selected_commit_oid.as_deref() {
                        ui.label(format!("Commit: {}", short_oid(commit)));
                    }
                    if let Some(branch) = state.snapshot.selection.selected_branch.as_deref() {
                        ui.label(format!("Branch: {branch}"));
                    }
                    ui.separator();
                    ui.label(format!("Staged: {}", state.snapshot.status.staged.len()));
                    ui.label(format!(
                        "Unstaged: {}",
                        state.snapshot.status.unstaged.len()
                    ));
                    ui.label(format!(
                        "Untracked: {}",
                        state.snapshot.status.untracked.len()
                    ));
                    if let Some(capabilities) = state.snapshot.repo_capabilities.as_ref() {
                        ui.separator();
                        ui.label("Capabilities");
                        ui.label(format!(
                            "Linked worktree: {}",
                            capabilities.is_linked_worktree
                        ));
                        ui.label(format!("Submodules: {}", capabilities.has_submodules));
                        ui.label(format!("LFS detected: {}", capabilities.lfs_detected));
                        ui.label(format!("LFS available: {}", capabilities.lfs_available));
                    }
                    if self.ui_state.layout.advanced_mode {
                        ui.separator();
                        ui.label(RichText::new("Context Actions").strong());
                        for item in state
                            .action_catalog
                            .iter()
                            .filter(|item| item.enabled)
                            .take(16)
                        {
                            ui.label(format!("{} ({})", item.action_id, item.owner));
                        }
                    }
                });
            });
    }

    fn render_status_bar(&mut self, root_ui: &mut egui::Ui, state: &RuntimeAdapterState) {
        egui::Panel::bottom("branchforge.status_bar").show_inside(root_ui, |ui| {
            ui.horizontal(|ui| {
                if state.busy {
                    ui.label(
                        state
                            .current_operation
                            .as_deref()
                            .unwrap_or("Operation running"),
                    );
                } else if let Some(error) = state.last_error.as_ref() {
                    ui.colored_label(Color32::from_rgb(248, 113, 113), error.to_string());
                } else if let Some(error) = self.ui_error.as_deref() {
                    ui.colored_label(Color32::from_rgb(248, 113, 113), error);
                    if ui.button("Clear").clicked() {
                        self.ui_error = None;
                    }
                } else if let Some(message) = state.last_message.as_deref() {
                    ui.label(message);
                } else {
                    ui.label("Ready");
                }
                ui.separator();
                ui.label(format!("State v{}", state.snapshot.version));
                ui.separator();
                ui.label(format!("Journal {}", state.snapshot.journal.entries.len()));
            });
        });
    }

    fn render_main_panel(&mut self, root_ui: &mut egui::Ui, state: &RuntimeAdapterState) {
        egui::CentralPanel::default().show_inside(root_ui, |ui| {
            egui::ScrollArea::vertical().show(ui, |ui| match self.ui_state.active_panel {
                PanelId::Status => self.render_status_panel(ui, state),
                PanelId::History => self.render_history_panel(ui, state),
                PanelId::Diff => self.render_diff_panel(ui, state),
                PanelId::Branches => self.render_branches_panel(ui, state),
                PanelId::Tags => self.render_tags_panel(ui, state),
                PanelId::Compare => self.render_compare_panel(ui, state),
                PanelId::Stash => self.render_simple_action_panel(
                    ui,
                    state,
                    "Stash",
                    &[("List stashes", "stash.list")],
                ),
                PanelId::Worktrees => self.render_simple_action_panel(
                    ui,
                    state,
                    "Worktrees",
                    &[("List worktrees", "worktree.list")],
                ),
                PanelId::Submodules => self.render_simple_action_panel(
                    ui,
                    state,
                    "Submodules",
                    &[("List submodules", "submodule.list")],
                ),
                PanelId::Conflicts => self.render_conflicts_panel(ui, state),
                PanelId::Diagnostics => self.render_diagnostics_panel(ui, state),
                PanelId::Journal => self.render_journal_panel(ui, state),
            });
        });
    }

    fn render_status_panel(&mut self, ui: &mut egui::Ui, state: &RuntimeAdapterState) {
        let snapshot = &state.snapshot;
        ui.horizontal(|ui| {
            ui.heading("Status");
            if let Some(conflict_state) = snapshot
                .repo
                .as_ref()
                .and_then(|repo| repo.conflict_state.as_ref())
            {
                ui.colored_label(
                    Color32::from_rgb(251, 191, 36),
                    format!("Conflict: {}", format_conflict_state(conflict_state)),
                );
            }
        });
        ui.separator();

        ui.columns(3, |columns| {
            self.render_file_bucket(
                &mut columns[0],
                "Staged",
                &snapshot.status.staged,
                &snapshot.selection.selected_paths,
                state.busy,
            );
            self.render_file_bucket(
                &mut columns[1],
                "Unstaged",
                &snapshot.status.unstaged,
                &snapshot.selection.selected_paths,
                state.busy,
            );
            self.render_file_bucket(
                &mut columns[2],
                "Untracked",
                &snapshot.status.untracked,
                &snapshot.selection.selected_paths,
                state.busy,
            );
        });

        ui.separator();
        ui.horizontal(|ui| {
            let has_selection = !snapshot.selection.selected_paths.is_empty();
            if ui
                .add_enabled(
                    has_selection && !state.busy,
                    egui::Button::new("Stage selected"),
                )
                .clicked()
            {
                self.execute_action_direct("index.stage_selected", Vec::new(), false);
            }
            if ui
                .add_enabled(
                    has_selection && !state.busy,
                    egui::Button::new("Unstage selected"),
                )
                .clicked()
            {
                self.execute_action_direct("index.unstage_selected", Vec::new(), false);
            }
            if ui
                .add_enabled(!state.busy, egui::Button::new("Refresh status"))
                .clicked()
            {
                self.execute_action_direct("status.refresh", Vec::new(), false);
            }
        });

        ui.separator();
        ui.label(RichText::new("Commit").strong());
        ui.add(
            egui::TextEdit::multiline(&mut self.commit_message)
                .desired_rows(3)
                .desired_width(f32::INFINITY),
        );
        let can_commit =
            !snapshot.status.staged.is_empty() && !self.commit_message.trim().is_empty();
        if ui
            .add_enabled(can_commit && !state.busy, egui::Button::new("Commit"))
            .clicked()
        {
            self.execute_action_direct(
                "commit.create",
                vec![self.commit_message.trim().to_string()],
                false,
            );
        }
    }

    fn render_file_bucket(
        &mut self,
        ui: &mut egui::Ui,
        title: &str,
        files: &[String],
        selected_paths: &[String],
        busy: bool,
    ) {
        ui.label(RichText::new(format!("{title} ({})", files.len())).strong());
        egui::ScrollArea::vertical()
            .max_height(240.0)
            .auto_shrink([false, false])
            .show(ui, |ui| {
                if files.is_empty() {
                    ui.weak("<empty>");
                    return;
                }
                for file in files {
                    let selected = selected_paths.iter().any(|path| path == file);
                    let response =
                        ui.add_enabled(!busy, egui::Button::selectable(selected, file.as_str()));
                    if response.clicked() {
                        let result = self.runtime.select_file(file);
                        self.record_submit(result);
                        self.ui_state.active_panel = PanelId::Diff;
                    }
                }
            });
    }

    fn render_history_panel(&mut self, ui: &mut egui::Ui, state: &RuntimeAdapterState) {
        let snapshot = &state.snapshot;
        ui.horizontal(|ui| {
            ui.heading("History Graph");
            if ui
                .add_enabled(!state.busy, egui::Button::new("Refresh history"))
                .clicked()
            {
                self.activate_panel(PanelId::History);
            }
            if ui
                .add_enabled(
                    snapshot.history.next_cursor.is_some() && !state.busy,
                    egui::Button::new("Load more"),
                )
                .clicked()
            {
                self.execute_action_direct("history.load_more", Vec::new(), false);
            }
        });
        ui.separator();

        if snapshot.history.loading {
            ui.spinner();
        }
        if let Some(error) = snapshot.history.error.as_deref() {
            ui.colored_label(Color32::from_rgb(248, 113, 113), error);
        }
        if snapshot.history.commits.is_empty() {
            ui.weak("<empty>");
            return;
        }
        let graph = if state.graph_rows.is_empty() {
            build_visible_graph(snapshot)
        } else {
            state.graph_rows.clone()
        };

        egui::Grid::new("history.rows")
            .striped(true)
            .min_col_width(90.0)
            .show(ui, |ui| {
                ui.label(RichText::new("Graph").strong());
                ui.label(RichText::new("Commit").strong());
                ui.label(RichText::new("Refs").strong());
                ui.label(RichText::new("Author").strong());
                ui.label(RichText::new("Time").strong());
                ui.end_row();

                for commit in &graph {
                    let selected = snapshot.selection.selected_commit_oid.as_deref()
                        == Some(commit.oid.as_str());
                    ui.label(RichText::new(graph_lane_text(commit)).monospace());
                    let response = ui.selectable_label(
                        selected,
                        format!("{} {}", short_oid(&commit.oid), commit.summary),
                    );
                    if response.clicked() {
                        self.ui_state.graph_view_state.selected_row = Some(commit.row);
                        let result = self.runtime.select_commit(&commit.oid);
                        self.record_submit(result);
                    }
                    response.context_menu(|ui| {
                        if ui.button("Copy oid").clicked() {
                            ui.ctx().copy_text(commit.oid.clone());
                            ui.close();
                        }
                        if ui
                            .add_enabled(!state.busy, egui::Button::new("Cherry-pick"))
                            .clicked()
                        {
                            self.execute_action_direct(
                                "cherry_pick.commit",
                                vec![commit.oid.clone()],
                                false,
                            );
                            ui.close();
                        }
                        if ui
                            .add_enabled(!state.busy, egui::Button::new("Revert"))
                            .clicked()
                        {
                            self.execute_action_direct(
                                "revert.commit",
                                vec![commit.oid.clone()],
                                false,
                            );
                            ui.close();
                        }
                    });
                    if commit.refs.is_empty() {
                        ui.label("");
                    } else {
                        ui.horizontal_wrapped(|ui| {
                            for label in &commit.refs {
                                ui.label(ref_label_text(label));
                            }
                        });
                    }
                    ui.label(commit.author.as_str());
                    ui.label(commit.time.as_str());
                    ui.end_row();
                }
            });
    }

    fn render_diff_panel(&mut self, ui: &mut egui::Ui, state: &RuntimeAdapterState) {
        let snapshot = &state.snapshot;
        ui.horizontal(|ui| {
            ui.heading("Diff");
            ui.selectable_value(
                &mut self.ui_state.diff_view_state.side_by_side,
                false,
                "Unified",
            );
            ui.selectable_value(
                &mut self.ui_state.diff_view_state.side_by_side,
                true,
                "Side by side",
            );
            if let Some(source) = snapshot.diff.source.as_ref() {
                ui.label(format_diff_source(source));
            }
        });
        ui.separator();

        if snapshot.diff.loading {
            ui.spinner();
        }
        if let Some(error) = snapshot.diff.error.as_deref() {
            ui.colored_label(Color32::from_rgb(248, 113, 113), error);
        }
        if snapshot.diff.hunks.is_empty() {
            if let Some(content) = snapshot.diff.content.as_deref() {
                render_diff_text(ui, content);
            } else {
                ui.weak("<no diff loaded>");
            }
            return;
        }

        self.render_diff_actions(ui, snapshot, state.busy);
        ui.separator();

        for (visible_index, hunk) in snapshot.diff.hunks.iter().enumerate() {
            let selected = self.ui_state.diff_view_state.selected_hunk == Some(visible_index);
            let label = format!("{} [{}] {}", hunk.file_path, hunk.hunk_index, hunk.header);
            if ui.selectable_label(selected, label).clicked() {
                if self.ui_state.diff_view_state.selected_hunk != Some(visible_index) {
                    self.ui_state.diff_view_state.selected_changed_lines.clear();
                }
                self.ui_state.diff_view_state.selected_hunk = Some(visible_index);
            }
            if selected {
                render_hunk_lines(
                    ui,
                    &hunk.lines,
                    self.ui_state.diff_view_state.side_by_side,
                    Some(&mut self.ui_state.diff_view_state.selected_changed_lines),
                );
            } else {
                render_hunk_lines(
                    ui,
                    &hunk.lines,
                    self.ui_state.diff_view_state.side_by_side,
                    None,
                );
            }
        }
    }

    fn render_diff_actions(&mut self, ui: &mut egui::Ui, snapshot: &StoreSnapshot, busy: bool) {
        let selected = self
            .ui_state
            .diff_view_state
            .selected_hunk
            .and_then(|index| snapshot.diff.hunks.get(index));
        let selected_lines = self.ui_state.diff_view_state.selected_changed_lines.clone();
        ui.horizontal(|ui| {
            if let Some(hunk) = selected {
                match snapshot.diff.source.as_ref() {
                    Some(DiffSource::Worktree { .. }) => {
                        if ui
                            .add_enabled(!busy, egui::Button::new("Stage hunk"))
                            .clicked()
                        {
                            self.execute_action_direct(
                                "index.stage_hunk",
                                vec![hunk.file_path.clone(), hunk.hunk_index.to_string()],
                                false,
                            );
                        }
                        if ui
                            .add_enabled(
                                !busy && !selected_lines.is_empty(),
                                egui::Button::new("Stage selected lines"),
                            )
                            .clicked()
                        {
                            let mut args =
                                vec![hunk.file_path.clone(), hunk.hunk_index.to_string()];
                            args.extend(selected_lines.iter().map(usize::to_string));
                            self.execute_action_direct("index.stage_lines", args, false);
                        }
                        if ui
                            .add_enabled(!busy, egui::Button::new("Discard hunk"))
                            .clicked()
                        {
                            self.ui_state.pending_confirmation = Some(ConfirmationDialog {
                                action_id: "file.discard_hunk".to_string(),
                                args: vec![hunk.file_path.clone(), hunk.hunk_index.to_string()],
                                title: "Confirm discard".to_string(),
                                message: "Discarding a hunk writes to the worktree.".to_string(),
                            });
                        }
                        if ui
                            .add_enabled(
                                !busy && !selected_lines.is_empty(),
                                egui::Button::new("Discard selected lines"),
                            )
                            .clicked()
                        {
                            let mut args =
                                vec![hunk.file_path.clone(), hunk.hunk_index.to_string()];
                            args.extend(selected_lines.iter().map(usize::to_string));
                            self.ui_state.pending_confirmation = Some(ConfirmationDialog {
                                action_id: "file.discard_lines".to_string(),
                                args,
                                title: "Confirm discard".to_string(),
                                message: "Discarding selected lines writes to the worktree."
                                    .to_string(),
                            });
                        }
                    }
                    Some(DiffSource::Index { .. }) => {
                        if ui
                            .add_enabled(!busy, egui::Button::new("Unstage hunk"))
                            .clicked()
                        {
                            self.execute_action_direct(
                                "index.unstage_hunk",
                                vec![hunk.file_path.clone(), hunk.hunk_index.to_string()],
                                false,
                            );
                        }
                        if ui
                            .add_enabled(
                                !busy && !selected_lines.is_empty(),
                                egui::Button::new("Unstage selected lines"),
                            )
                            .clicked()
                        {
                            let mut args =
                                vec![hunk.file_path.clone(), hunk.hunk_index.to_string()];
                            args.extend(selected_lines.iter().map(usize::to_string));
                            self.execute_action_direct("index.unstage_lines", args, false);
                        }
                    }
                    Some(DiffSource::Commit { .. }) | Some(DiffSource::Compare { .. }) | None => {
                        ui.weak("Read-only diff");
                    }
                }
            } else {
                ui.weak("Select a hunk");
            }
        });
    }

    fn render_branches_panel(&mut self, ui: &mut egui::Ui, state: &RuntimeAdapterState) {
        let snapshot = &state.snapshot;
        ui.horizontal(|ui| {
            ui.heading("Branches");
            if ui
                .add_enabled(!state.busy, egui::Button::new("Refresh refs"))
                .clicked()
            {
                self.execute_action_direct("refs.refresh", Vec::new(), false);
            }
        });
        ui.separator();

        ui.horizontal(|ui| {
            ui.add(
                egui::TextEdit::singleline(&mut self.branch_name_input)
                    .desired_width(220.0)
                    .hint_text("Branch name"),
            );
            if ui
                .add_enabled(
                    !self.branch_name_input.trim().is_empty() && !state.busy,
                    egui::Button::new("Create"),
                )
                .clicked()
            {
                self.execute_action_direct(
                    "branch.create",
                    vec![self.branch_name_input.trim().to_string()],
                    false,
                );
            }
            if let Some(branch) = snapshot.selection.selected_branch.as_deref() {
                if ui
                    .add_enabled(!state.busy, egui::Button::new("Checkout"))
                    .clicked()
                {
                    self.execute_action_direct("branch.checkout", vec![branch.to_string()], false);
                }
                if ui
                    .add_enabled(!state.busy, egui::Button::new("Delete"))
                    .clicked()
                {
                    self.ui_state.pending_confirmation = Some(ConfirmationDialog {
                        action_id: "branch.delete".to_string(),
                        args: vec![branch.to_string()],
                        title: "Confirm branch delete".to_string(),
                        message: format!("Delete branch {branch}?"),
                    });
                }
            }
        });

        ui.separator();
        if snapshot.branches.branches.is_empty() {
            ui.weak("<empty>");
            return;
        }

        egui::Grid::new("branches.rows")
            .striped(true)
            .show(ui, |ui| {
                ui.label(RichText::new("Name").strong());
                ui.label(RichText::new("Upstream").strong());
                ui.end_row();
                for branch in &snapshot.branches.branches {
                    let selected =
                        snapshot.selection.selected_branch.as_deref() == Some(branch.name.as_str());
                    let label = if branch.is_current {
                        format!("{} [current]", branch.name)
                    } else {
                        branch.name.clone()
                    };
                    if ui.selectable_label(selected, label).clicked() {
                        let result = self.runtime.select_branch(&branch.name);
                        self.record_submit(result);
                    }
                    ui.label(branch.upstream.as_deref().unwrap_or(""));
                    ui.end_row();
                }
            });
    }

    fn render_tags_panel(&mut self, ui: &mut egui::Ui, state: &RuntimeAdapterState) {
        ui.heading("Tags");
        ui.separator();
        if state.snapshot.tags.tags.is_empty() {
            ui.weak("<empty>");
            return;
        }

        for tag in &state.snapshot.tags.tags {
            ui.horizontal(|ui| {
                ui.label(tag.name.as_str());
                if ui
                    .add_enabled(!state.busy, egui::Button::new("Checkout"))
                    .clicked()
                {
                    self.execute_action_direct("tag.checkout", vec![tag.name.clone()], false);
                }
                if ui
                    .add_enabled(!state.busy, egui::Button::new("Delete"))
                    .clicked()
                {
                    self.ui_state.pending_confirmation = Some(ConfirmationDialog {
                        action_id: "tag.delete".to_string(),
                        args: vec![tag.name.clone()],
                        title: "Confirm tag delete".to_string(),
                        message: format!("Delete tag {}?", tag.name),
                    });
                }
            });
        }
    }

    fn render_compare_panel(&mut self, ui: &mut egui::Ui, state: &RuntimeAdapterState) {
        let snapshot = &state.snapshot;
        ui.heading("Compare");
        ui.separator();
        ui.horizontal(|ui| {
            ui.add(
                egui::TextEdit::singleline(&mut self.compare_base_input)
                    .desired_width(180.0)
                    .hint_text("Base ref"),
            );
            ui.add(
                egui::TextEdit::singleline(&mut self.compare_head_input)
                    .desired_width(180.0)
                    .hint_text("Head ref"),
            );
            if ui
                .add_enabled(
                    !self.compare_base_input.trim().is_empty()
                        && !self.compare_head_input.trim().is_empty()
                        && !state.busy,
                    egui::Button::new("Compare"),
                )
                .clicked()
            {
                self.execute_action_direct(
                    "compare.refs",
                    vec![
                        self.compare_base_input.trim().to_string(),
                        self.compare_head_input.trim().to_string(),
                    ],
                    false,
                );
            }
        });
        ui.separator();
        ui.label(format!(
            "Current: {} -> {}",
            snapshot.compare.base_ref.as_deref().unwrap_or("<unset>"),
            snapshot.compare.head_ref.as_deref().unwrap_or("<unset>")
        ));
        ui.label(format!(
            "Ahead/behind: +{} / -{}",
            snapshot.compare.ahead, snapshot.compare.behind
        ));
        for commit in snapshot.compare.commits.iter().take(30) {
            ui.label(format!("{} {}", short_oid(&commit.oid), commit.summary));
        }
    }

    fn render_simple_action_panel(
        &mut self,
        ui: &mut egui::Ui,
        state: &RuntimeAdapterState,
        title: &str,
        actions: &[(&str, &str)],
    ) {
        ui.heading(title);
        ui.separator();
        ui.horizontal(|ui| {
            for (label, action_id) in actions {
                if ui
                    .add_enabled(!state.busy, egui::Button::new(*label))
                    .clicked()
                {
                    self.execute_action_direct(action_id, Vec::new(), false);
                }
            }
        });
        ui.separator();
        if let Some(content) = state.snapshot.diff.content.as_deref() {
            render_diff_text(ui, content);
        } else {
            ui.weak("<empty>");
        }
    }

    fn render_conflicts_panel(&mut self, ui: &mut egui::Ui, state: &RuntimeAdapterState) {
        let snapshot = &state.snapshot;
        ui.heading("Conflicts");
        ui.separator();
        match snapshot
            .repo
            .as_ref()
            .and_then(|repo| repo.conflict_state.as_ref())
        {
            Some(conflict_state) => {
                ui.colored_label(
                    Color32::from_rgb(251, 191, 36),
                    format!("Session: {}", format_conflict_state(conflict_state)),
                );
            }
            None => {
                ui.weak("No active conflict session.");
            }
        }
        let unresolved_markers = snapshot
            .diff
            .content
            .as_deref()
            .map(count_conflict_markers)
            .unwrap_or(0);
        self.ui_state.conflict_view_state.unresolved_count = unresolved_markers;
        if unresolved_markers > 0 {
            ui.colored_label(
                Color32::from_rgb(251, 191, 36),
                format!("{unresolved_markers} conflict marker lines visible"),
            );
        }

        ui.horizontal(|ui| {
            if ui
                .add_enabled(!state.busy, egui::Button::new("List conflicts"))
                .clicked()
            {
                self.execute_action_direct("conflict.list", Vec::new(), false);
            }
            if let Some(file) = self.selected_file(snapshot) {
                if ui
                    .add_enabled(!state.busy, egui::Button::new("Focus"))
                    .clicked()
                {
                    self.execute_action_direct("conflict.focus", vec![file.to_string()], false);
                }
                if ui
                    .add_enabled(!state.busy, egui::Button::new("Use ours"))
                    .clicked()
                {
                    self.execute_action_direct(
                        "conflict.resolve.ours",
                        vec![file.to_string()],
                        false,
                    );
                }
                if ui
                    .add_enabled(!state.busy, egui::Button::new("Use theirs"))
                    .clicked()
                {
                    self.execute_action_direct(
                        "conflict.resolve.theirs",
                        vec![file.to_string()],
                        false,
                    );
                }
                if ui
                    .add_enabled(!state.busy, egui::Button::new("Mark resolved"))
                    .clicked()
                {
                    self.execute_action_direct(
                        "conflict.mark_resolved",
                        vec![file.to_string()],
                        false,
                    );
                }
            }
            if ui
                .add_enabled(!state.busy, egui::Button::new("Continue"))
                .clicked()
            {
                if unresolved_markers > 0 {
                    self.ui_state.pending_confirmation = Some(ConfirmationDialog {
                        action_id: "conflict.continue".to_string(),
                        args: Vec::new(),
                        title: "Continue with markers?".to_string(),
                        message: format!(
                            "{unresolved_markers} conflict marker lines are still visible. Continue anyway?"
                        ),
                    });
                } else {
                    self.execute_action_direct("conflict.continue", Vec::new(), false);
                }
            }
            if ui
                .add_enabled(!state.busy, egui::Button::new("Abort"))
                .clicked()
            {
                self.ui_state.pending_confirmation = Some(ConfirmationDialog {
                    action_id: "conflict.abort".to_string(),
                    args: Vec::new(),
                    title: "Confirm abort".to_string(),
                    message: "Abort the active conflict session?".to_string(),
                });
            }
        });

        ui.separator();
        for path in &snapshot.selection.selected_paths {
            let selected =
                self.ui_state.conflict_view_state.selected_path.as_deref() == Some(path.as_str());
            if ui.selectable_label(selected, path).clicked() {
                self.ui_state.conflict_view_state.selected_path = Some(path.clone());
                let result = self.runtime.select_file(path);
                self.record_submit(result);
            }
        }
        if let Some(content) = snapshot.diff.content.as_deref() {
            ui.separator();
            render_diff_text(ui, content);
        }
    }

    fn render_diagnostics_panel(&mut self, ui: &mut egui::Ui, state: &RuntimeAdapterState) {
        ui.heading("Diagnostics");
        ui.separator();
        ui.label(format!("Action catalog: {}", state.action_catalog.len()));
        ui.label(format!("Plugins: {}", state.snapshot.plugins.len()));
        for plugin in &state.snapshot.plugins {
            ui.label(format!("{}: {:?}", plugin.plugin_id, plugin.health));
        }
        if !state.snapshot.installed_plugins.is_empty() {
            ui.separator();
            ui.label(RichText::new("Installed Plugins").strong());
            for plugin in &state.snapshot.installed_plugins {
                ui.label(format!(
                    "{} v{} {}",
                    plugin.plugin_id,
                    plugin.version,
                    if plugin.enabled {
                        "enabled"
                    } else {
                        "disabled"
                    }
                ));
            }
        }
        ui.separator();
        ui.label(RichText::new("Actions").strong());
        egui::Grid::new("action.catalog")
            .striped(true)
            .min_col_width(80.0)
            .show(ui, |ui| {
                ui.label(RichText::new("State").strong());
                ui.label(RichText::new("Action").strong());
                ui.label(RichText::new("Owner").strong());
                ui.label(RichText::new("Danger").strong());
                ui.end_row();
                for item in &state.action_catalog {
                    ui.label(if item.enabled { "on" } else { "off" });
                    ui.label(item.action_id.as_str());
                    ui.label(item.owner.as_str());
                    ui.label(format_danger(&item.danger));
                    ui.end_row();
                }
            });
    }

    fn render_journal_panel(&mut self, ui: &mut egui::Ui, state: &RuntimeAdapterState) {
        ui.heading("Journal");
        ui.separator();
        if state.snapshot.journal.entries.is_empty() {
            ui.weak("<empty>");
            return;
        }
        for entry in state.snapshot.journal.entries.iter().rev() {
            render_journal_entry(ui, entry);
            ui.separator();
        }
    }

    fn render_command_palette(&mut self, ctx: &egui::Context, state: &RuntimeAdapterState) {
        if !self.ui_state.command_palette.open {
            return;
        }

        let mut open = self.ui_state.command_palette.open;
        let mut close_after_select = false;
        egui::Window::new("Command Palette")
            .open(&mut open)
            .collapsible(false)
            .resizable(true)
            .default_width(620.0)
            .show(ctx, |ui| {
                ui.add(
                    egui::TextEdit::singleline(&mut self.ui_state.command_palette.filter)
                        .hint_text("Search actions")
                        .desired_width(f32::INFINITY),
                );
                ui.add(
                    egui::TextEdit::singleline(&mut self.ui_state.command_palette.args)
                        .hint_text("Arguments")
                        .desired_width(f32::INFINITY),
                );
                ui.separator();

                let filter = self.ui_state.command_palette.filter.to_lowercase();
                let mut items = state
                    .action_catalog
                    .iter()
                    .filter(|item| palette_matches(item, &filter))
                    .collect::<Vec<_>>();
                items.sort_by_key(|item| palette_rank(item, &filter));

                egui::ScrollArea::vertical()
                    .max_height(420.0)
                    .show(ui, |ui| {
                        for item in items.into_iter().take(80) {
                            let enabled = item.enabled && !state.busy;
                            ui.horizontal(|ui| {
                                let response = ui.add_enabled(
                                    enabled,
                                    egui::Button::new(format!(
                                        "{}   {}",
                                        item.title, item.action_id
                                    )),
                                );
                                if response.clicked() {
                                    self.run_palette_item(item);
                                    close_after_select = true;
                                }
                                ui.label(item.owner.as_str());
                                ui.label(format_danger(&item.danger));
                                if let Some(reason) = item.disabled_reason.as_deref() {
                                    ui.weak(reason);
                                }
                            });
                        }
                    });
            });

        if close_after_select {
            open = false;
        }
        self.ui_state.command_palette.open = open;
    }

    fn run_palette_item(&mut self, item: &HostActionCatalogItem) {
        let args = match split_palette_args(&self.ui_state.command_palette.args) {
            Ok(args) => args,
            Err(error) => {
                self.ui_error = Some(error);
                return;
            }
        };

        if item.action_id == "repo.open" {
            let path = args
                .first()
                .cloned()
                .unwrap_or_else(|| self.repo_path_input.trim().to_string());
            if path.is_empty() {
                self.ui_error = Some("repo.open needs a repository path.".to_string());
                return;
            }
            let result = self.runtime.open_repo(path);
            self.record_submit(result);
            return;
        }

        self.execute_or_confirm(item, args, format!("Confirm {}", item.action_id));
    }

    fn render_confirmation(&mut self, ctx: &egui::Context) {
        let Some(dialog) = self.ui_state.pending_confirmation.clone() else {
            return;
        };

        let mut keep_open = true;
        egui::Window::new(dialog.title.as_str())
            .open(&mut keep_open)
            .collapsible(false)
            .resizable(false)
            .show(ctx, |ui| {
                ui.label(dialog.message.as_str());
                ui.separator();
                ui.horizontal(|ui| {
                    if ui.button("Cancel").clicked() {
                        self.ui_state.pending_confirmation = None;
                    }
                    if ui.button("Confirm").clicked() {
                        let result =
                            self.runtime
                                .execute_action(&dialog.action_id, &dialog.args, true);
                        self.record_submit(result);
                        self.ui_state.pending_confirmation = None;
                    }
                });
            });

        if !keep_open {
            self.ui_state.pending_confirmation = None;
        }
    }
}

impl eframe::App for BranchForgeDesktopApp {
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        let ctx = ui.ctx().clone();
        self.handle_shortcuts(&ctx);
        let state = self.runtime.state();
        self.render_top_bar(ui, &state);
        self.render_sidebar(ui);
        self.render_inspector(ui, &state);
        self.render_status_bar(ui, &state);
        self.render_main_panel(ui, &state);
        self.render_command_palette(&ctx, &state);
        self.render_confirmation(&ctx);

        if state.busy {
            ctx.request_repaint_after(Duration::from_millis(80));
        } else {
            ctx.request_repaint_after(Duration::from_millis(350));
        }
    }
}

fn action_requires_confirmation(item: &HostActionCatalogItem) -> bool {
    match item.confirm_policy {
        ConfirmPolicy::Never => false,
        ConfirmPolicy::Always => true,
        ConfirmPolicy::OnDanger => matches!(item.danger, DangerLevel::High),
    }
}

fn format_danger(danger: &DangerLevel) -> &'static str {
    match danger {
        DangerLevel::Low => "low",
        DangerLevel::Medium => "medium",
        DangerLevel::High => "high",
    }
}

fn format_conflict_state(state: &plugin_api::ConflictState) -> &'static str {
    match state {
        plugin_api::ConflictState::Merge => "merge",
        plugin_api::ConflictState::Rebase => "rebase",
        plugin_api::ConflictState::CherryPick => "cherry-pick",
    }
}

fn format_diff_source(source: &DiffSource) -> String {
    match source {
        DiffSource::Worktree { paths } => format!("Worktree {}", paths.join(", ")),
        DiffSource::Index { paths } => format!("Index {}", paths.join(", ")),
        DiffSource::Commit { oid } => format!("Commit {}", short_oid(oid)),
        DiffSource::Compare { base, head } => format!("Compare {base}..{head}"),
    }
}

fn short_oid(oid: &str) -> String {
    oid.chars().take(8).collect()
}

fn build_visible_graph(snapshot: &StoreSnapshot) -> Vec<GraphCommit> {
    let inputs = snapshot
        .history
        .commits
        .iter()
        .enumerate()
        .map(|(index, commit)| {
            let parents = snapshot
                .history
                .commits
                .get(index + 1)
                .map(|parent| vec![parent.oid.clone()])
                .unwrap_or_default();
            GraphInputCommit {
                oid: commit.oid.clone(),
                short_oid: short_oid(&commit.oid),
                summary: commit.summary.clone(),
                author: commit.author.clone(),
                time: commit.time.clone(),
                parents,
            }
        })
        .collect::<Vec<_>>();

    let mut refs = Vec::new();
    if let (Some(first_commit), Some(head)) = (
        snapshot.history.commits.first(),
        snapshot.repo.as_ref().and_then(|repo| repo.head.as_ref()),
    ) {
        refs.push(GraphRef {
            oid: first_commit.oid.clone(),
            label: GraphRefLabel {
                name: head.clone(),
                kind: GraphRefKind::Head,
            },
        });
    }

    build_graph(&inputs, &refs)
}

fn graph_lane_text(commit: &GraphCommit) -> String {
    let max_lane = commit
        .edges
        .iter()
        .fold(commit.lane, |max_lane, edge| {
            max_lane.max(edge.from_lane).max(edge.to_lane)
        })
        .min(5);
    let mut cells = Vec::new();
    for lane in 0..=max_lane {
        if lane == commit.lane {
            cells.push("o");
        } else if commit
            .edges
            .iter()
            .any(|edge| edge.from_lane == lane || edge.to_lane == lane)
        {
            cells.push("|");
        } else {
            cells.push(" ");
        }
    }
    cells.join(" ")
}

fn ref_label_text(label: &GraphRefLabel) -> RichText {
    let color = match label.kind {
        GraphRefKind::Head | GraphRefKind::LocalBranch => Color32::from_rgb(125, 211, 252),
        GraphRefKind::RemoteBranch => Color32::from_rgb(167, 139, 250),
        GraphRefKind::Tag => Color32::from_rgb(250, 204, 21),
    };
    RichText::new(format!("[{}]", label.name)).color(color)
}

fn render_hunk_lines(
    ui: &mut egui::Ui,
    lines: &[String],
    side_by_side: bool,
    mut selected_changed_lines: Option<&mut Vec<usize>>,
) {
    if side_by_side {
        egui::Grid::new(("side-by-side", lines.as_ptr() as usize, lines.len()))
            .num_columns(2)
            .striped(true)
            .show(ui, |ui| {
                let mut changed_index = 0usize;
                for (index, line) in lines.iter().enumerate() {
                    if index >= MAX_RENDERED_DIFF_LINES {
                        ui.weak("<truncated>");
                        ui.weak("<truncated>");
                        ui.end_row();
                        break;
                    }
                    let selectable_index = if is_changed_diff_line(line) {
                        let current = changed_index;
                        changed_index += 1;
                        Some(current)
                    } else {
                        None
                    };
                    if line.starts_with('-') {
                        render_selectable_diff_line(
                            ui,
                            line,
                            selectable_index,
                            &mut selected_changed_lines,
                        );
                        ui.label("");
                    } else if line.starts_with('+') {
                        ui.label("");
                        render_selectable_diff_line(
                            ui,
                            line,
                            selectable_index,
                            &mut selected_changed_lines,
                        );
                    } else {
                        ui.label(diff_line_text(line));
                        ui.label(diff_line_text(line));
                    }
                    ui.end_row();
                }
            });
        return;
    }

    let mut changed_index = 0usize;
    for (index, line) in lines.iter().enumerate() {
        if index >= MAX_RENDERED_DIFF_LINES {
            ui.weak("<diff truncated in view>");
            break;
        }
        let selectable_index = if is_changed_diff_line(line) {
            let current = changed_index;
            changed_index += 1;
            Some(current)
        } else {
            None
        };
        render_selectable_diff_line(ui, line, selectable_index, &mut selected_changed_lines);
    }
}

fn render_selectable_diff_line(
    ui: &mut egui::Ui,
    line: &str,
    changed_index: Option<usize>,
    selected_changed_lines: &mut Option<&mut Vec<usize>>,
) {
    let Some(changed_index) = changed_index else {
        ui.label(diff_line_text(line));
        return;
    };
    let Some(selected_lines) = selected_changed_lines.as_mut() else {
        ui.label(diff_line_text(line));
        return;
    };
    let selected_lines = &mut **selected_lines;
    let selected = selected_lines.contains(&changed_index);
    if ui
        .selectable_label(selected, diff_line_text(line))
        .on_hover_text(format!("changed line index {changed_index}"))
        .clicked()
    {
        if selected {
            selected_lines.retain(|line_index| *line_index != changed_index);
        } else {
            selected_lines.push(changed_index);
            selected_lines.sort_unstable();
        }
    }
}

fn is_changed_diff_line(line: &str) -> bool {
    (line.starts_with('+') && !line.starts_with("+++"))
        || (line.starts_with('-') && !line.starts_with("---"))
}

fn render_diff_text(ui: &mut egui::Ui, content: &str) {
    for (index, line) in content.lines().enumerate() {
        if index >= MAX_RENDERED_DIFF_LINES {
            ui.weak("<diff truncated in view>");
            break;
        }
        ui.label(diff_line_text(line));
    }
}

fn diff_line_text(line: &str) -> RichText {
    let color = if is_conflict_marker_line(line) {
        Color32::from_rgb(251, 191, 36)
    } else if line.starts_with('+') {
        Color32::from_rgb(74, 222, 128)
    } else if line.starts_with('-') {
        Color32::from_rgb(248, 113, 113)
    } else if line.starts_with("@@") {
        Color32::from_rgb(125, 211, 252)
    } else {
        Color32::from_rgb(209, 213, 219)
    };
    RichText::new(line.to_string()).monospace().color(color)
}

fn count_conflict_markers(content: &str) -> usize {
    content
        .lines()
        .filter(|line| is_conflict_marker_line(line))
        .count()
}

fn is_conflict_marker_line(line: &str) -> bool {
    let trimmed = line.trim_start_matches(['+', '-', ' ']).trim_start();
    trimmed.starts_with("<<<<<<<")
        || trimmed.starts_with("=======")
        || trimmed.starts_with(">>>>>>>")
}

fn render_journal_entry(ui: &mut egui::Ui, entry: &OperationJournalEntry) {
    let color = match entry.status {
        JournalStatus::Started => Color32::from_rgb(125, 211, 252),
        JournalStatus::Succeeded => Color32::from_rgb(74, 222, 128),
        JournalStatus::Failed => Color32::from_rgb(248, 113, 113),
    };
    ui.horizontal(|ui| {
        ui.colored_label(color, format!("{:?}", entry.status));
        ui.label(format!("#{} {}", entry.id, entry.op));
        if let Some(job_id) = entry.job_id {
            ui.label(format!("job {job_id}"));
        }
    });
    if let Some(error) = entry.error.as_deref() {
        ui.colored_label(Color32::from_rgb(248, 113, 113), error);
    }
    if let Some(pre_refs) = entry.pre_refs.as_ref() {
        ui.label(format!(
            "pre refs: head={} branches={} tags={}",
            pre_refs.head.as_deref().unwrap_or("<none>"),
            pre_refs.branch_count,
            pre_refs.tag_count
        ));
    }
    if let Some(post_refs) = entry.post_refs.as_ref() {
        ui.label(format!(
            "post refs: head={} branches={} tags={}",
            post_refs.head.as_deref().unwrap_or("<none>"),
            post_refs.branch_count,
            post_refs.tag_count
        ));
    }
}

fn palette_matches(item: &HostActionCatalogItem, filter: &str) -> bool {
    filter.is_empty()
        || item.action_id.to_lowercase().contains(filter)
        || item.title.to_lowercase().contains(filter)
        || item.owner.to_lowercase().contains(filter)
}

fn palette_rank(item: &HostActionCatalogItem, filter: &str) -> usize {
    if filter.is_empty() {
        return 5;
    }
    let action = item.action_id.to_lowercase();
    let title = item.title.to_lowercase();
    if action == filter {
        0
    } else if action.starts_with(filter) {
        1
    } else if title.starts_with(filter) {
        2
    } else if title.contains(filter) {
        3
    } else {
        4
    }
}

fn split_palette_args(raw: &str) -> Result<Vec<String>, String> {
    let mut tokens = Vec::new();
    let mut current = String::new();
    let mut chars = raw.trim().chars().peekable();
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
        return Err("unterminated quote in command arguments".to_string());
    }
    if !current.is_empty() {
        tokens.push(current);
    }
    Ok(tokens)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn palette_args_preserve_quoted_segments() {
        let parsed = split_palette_args("feature/main \"docs/read me.md\"");
        assert!(parsed.is_ok());
        if let Ok(args) = parsed {
            assert_eq!(
                args,
                vec!["feature/main".to_string(), "docs/read me.md".to_string()]
            );
        }
    }

    #[test]
    fn visible_history_builds_graph_rows_with_head_label() {
        let snapshot = StoreSnapshot {
            repo: Some(plugin_api::RepoSnapshot {
                root: "/tmp/repo".to_string(),
                head: Some("main".to_string()),
                conflict_state: None,
            }),
            history: state_store::HistoryState {
                commits: vec![
                    state_store::CommitSummary {
                        oid: "abcdef123456".to_string(),
                        author: "Dev".to_string(),
                        time: "now".to_string(),
                        summary: "tip".to_string(),
                    },
                    state_store::CommitSummary {
                        oid: "123456abcdef".to_string(),
                        author: "Dev".to_string(),
                        time: "then".to_string(),
                        summary: "parent".to_string(),
                    },
                ],
                ..state_store::HistoryState::default()
            },
            ..StoreSnapshot::default()
        };

        let graph = build_visible_graph(&snapshot);

        assert_eq!(graph.len(), 2);
        assert_eq!(graph[0].lane, 0);
        assert_eq!(graph[0].edges[0].to_oid, "123456abcdef");
        assert_eq!(graph[0].refs[0].name, "main");
    }

    #[test]
    fn conflict_marker_counter_handles_diff_prefixes() {
        let content = "\
+<<<<<<< HEAD
unchanged
+=======
+>>>>>>> topic
";

        assert_eq!(count_conflict_markers(content), 3);
    }

    #[test]
    fn smoke_launch_initializes_runtime_catalog() {
        let result = smoke_launch();
        assert!(result.is_ok());
        if let Ok(message) = result {
            assert!(message.contains("desktop smoke launch ok"));
        }
    }
}
