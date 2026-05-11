pub mod runtime_adapter;
pub mod ui_state;

use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::Duration;

use app_host::HostActionCatalogItem;
use eframe::egui::{self, Color32, RichText};
use graph_model::{
    GraphCommit, GraphInputCommit, GraphRef, GraphRefKind, GraphRefLabel, build_graph,
};
use plugin_api::{ConfirmPolicy, DangerLevel};
use runtime_adapter::{DesktopRuntimeAdapter, DesktopRuntimeError, RuntimeAdapterState};
use state_store::{
    DiffSource, ImpactLevel, JournalStatus, OperationJournalEntry, OperationPreview,
    PreviewWarningLevel, StoreSnapshot,
};
use ui_state::{ConfirmationDialog, DesktopUiState, PanelId, PanelStatus, PreviewDialog};

const MAX_RENDERED_DIFF_LINES: usize = 900;

pub mod design_tokens {
    use eframe::egui::Color32;

    pub const SPACING_XS: f32 = 4.0;
    pub const SPACING_SM: f32 = 8.0;
    pub const SPACING_MD: f32 = 12.0;
    pub const SPACING_LG: f32 = 16.0;
    pub const SPACING_XL: f32 = 24.0;
    pub const SPACING_2XL: f32 = 32.0;

    pub const SIDEBAR_WIDTH: f32 = 210.0;
    pub const INSPECTOR_WIDTH: f32 = 280.0;
    pub const TOPBAR_HEIGHT: f32 = 58.0;
    pub const BOTTOMBAR_HEIGHT: f32 = 26.0;

    pub const ROW_HEIGHT_SM: f32 = 24.0;
    pub const ROW_HEIGHT_MD: f32 = 30.0;
    pub const ROW_HEIGHT_LG: f32 = 36.0;

    pub const BUTTON_HEIGHT_SM: f32 = 24.0;
    pub const BUTTON_HEIGHT_MD: f32 = 28.0;
    pub const INPUT_HEIGHT_MD: f32 = 28.0;

    pub const RADIUS_SM: f32 = 4.0;
    pub const RADIUS_MD: f32 = 6.0;
    pub const RADIUS_LG: f32 = 8.0;

    pub const FONT_SIZE_XS: f32 = 11.0;
    pub const FONT_SIZE_SM: f32 = 12.0;
    pub const FONT_SIZE_MD: f32 = 13.0;
    pub const FONT_SIZE_LG: f32 = 15.0;
    pub const FONT_SIZE_TITLE: f32 = 18.0;

    pub fn bg(dark: bool) -> Color32 {
        if dark {
            Color32::from_rgb(23, 26, 29)
        } else {
            Color32::from_rgb(246, 247, 248)
        }
    }

    pub fn surface(dark: bool) -> Color32 {
        if dark {
            Color32::from_rgb(32, 36, 40)
        } else {
            Color32::WHITE
        }
    }

    pub fn surface_alt(dark: bool) -> Color32 {
        if dark {
            Color32::from_rgb(41, 46, 51)
        } else {
            Color32::from_rgb(240, 242, 244)
        }
    }

    pub fn surface_hover(dark: bool) -> Color32 {
        if dark {
            Color32::from_rgb(48, 54, 61)
        } else {
            Color32::from_rgb(233, 238, 242)
        }
    }

    pub fn surface_active(dark: bool) -> Color32 {
        if dark {
            Color32::from_rgb(23, 50, 74)
        } else {
            Color32::from_rgb(220, 238, 255)
        }
    }

    pub fn border(dark: bool) -> Color32 {
        if dark {
            Color32::from_rgb(58, 64, 71)
        } else {
            Color32::from_rgb(214, 218, 223)
        }
    }

    pub fn border_strong(dark: bool) -> Color32 {
        if dark {
            Color32::from_rgb(75, 85, 96)
        } else {
            Color32::from_rgb(184, 192, 200)
        }
    }

    pub fn text(dark: bool) -> Color32 {
        if dark {
            Color32::from_rgb(229, 231, 235)
        } else {
            Color32::from_rgb(31, 35, 40)
        }
    }

    pub fn text_muted(dark: bool) -> Color32 {
        if dark {
            Color32::from_rgb(160, 167, 176)
        } else {
            Color32::from_rgb(107, 114, 128)
        }
    }

    pub fn text_subtle(dark: bool) -> Color32 {
        if dark {
            Color32::from_rgb(122, 130, 140)
        } else {
            Color32::from_rgb(138, 146, 156)
        }
    }

    pub fn accent(dark: bool) -> Color32 {
        if dark {
            Color32::from_rgb(77, 163, 230)
        } else {
            Color32::from_rgb(23, 139, 214)
        }
    }

    pub fn accent_soft(dark: bool) -> Color32 {
        if dark {
            Color32::from_rgb(23, 50, 74)
        } else {
            Color32::from_rgb(220, 238, 255)
        }
    }

    pub fn success(dark: bool) -> Color32 {
        if dark {
            Color32::from_rgb(66, 184, 131)
        } else {
            Color32::from_rgb(31, 157, 85)
        }
    }

    pub fn warning(dark: bool) -> Color32 {
        if dark {
            Color32::from_rgb(214, 168, 79)
        } else {
            Color32::from_rgb(183, 121, 31)
        }
    }

    pub fn danger(dark: bool) -> Color32 {
        if dark {
            Color32::from_rgb(255, 107, 107)
        } else {
            Color32::from_rgb(214, 69, 69)
        }
    }

    pub fn danger_soft(dark: bool) -> Color32 {
        if dark {
            Color32::from_rgb(74, 31, 31)
        } else {
            Color32::from_rgb(255, 225, 225)
        }
    }

    pub fn disabled(dark: bool) -> Color32 {
        if dark {
            Color32::from_rgb(107, 114, 128)
        } else {
            Color32::from_rgb(168, 175, 183)
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ActionButtonKind {
    Primary,
    Secondary,
    Danger,
    Ghost,
}

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
    commit_description: String,
    branch_name_input: String,
    compare_base_input: String,
    compare_head_input: String,
    remote_name_input: String,
    remote_url_input: String,
    auth_host_input: String,
    auth_username_input: String,
    auth_token_input: String,
    show_credentials_dialog: bool,
    workspace_name_input: String,
    workspace_repo_input: String,
    pr_base_input: String,
    pr_head_input: String,
    pr_title_input: String,
    pr_number_input: String,
    plugin_registry_input: String,
    plugin_id_input: String,
    stack_name_input: String,
    stack_base_input: String,
    stack_branches_input: String,
    stash_message_input: String,
    stash_selector_input: String,
    virtual_branch_name_input: String,
    virtual_branch_paths_input: String,
    ui_error: Option<String>,
    pending_commit_clear_after_version: Option<u64>,
    last_synced_repo_root: Option<String>,
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
            commit_description: String::new(),
            branch_name_input: String::new(),
            compare_base_input: String::new(),
            compare_head_input: String::new(),
            remote_name_input: "origin".to_string(),
            remote_url_input: String::new(),
            auth_host_input: "github.com".to_string(),
            auth_username_input: String::new(),
            auth_token_input: String::new(),
            show_credentials_dialog: false,
            workspace_name_input: "Default Workspace".to_string(),
            workspace_repo_input: String::new(),
            pr_base_input: "main".to_string(),
            pr_head_input: String::new(),
            pr_title_input: String::new(),
            pr_number_input: String::new(),
            plugin_registry_input: "plugin_registry/registry.json".to_string(),
            plugin_id_input: String::new(),
            stack_name_input: "Feature Stack".to_string(),
            stack_base_input: "main".to_string(),
            stack_branches_input: String::new(),
            stash_message_input: "WIP from BranchForge".to_string(),
            stash_selector_input: "stash@{0}".to_string(),
            virtual_branch_name_input: "Working changes".to_string(),
            virtual_branch_paths_input: String::new(),
            ui_error,
            pending_commit_clear_after_version: None,
            last_synced_repo_root: None,
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
            self.ui_error = Some("Enter a repository path before opening.".to_string());
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
        match result {
            Ok(()) => {
                self.ui_error = None;
            }
            Err(error) => {
                self.ui_error = Some(error.to_string());
            }
        }
    }

    fn execute_or_confirm(
        &mut self,
        item: &HostActionCatalogItem,
        args: Vec<String>,
        title: impl Into<String>,
    ) {
        if action_requires_confirmation(item) {
            let action_id = item.action_id.clone();
            self.preview_or_confirm(
                action_id.as_str(),
                args,
                title.into(),
                format!(
                    "{} requires confirmation because its danger level is {}.",
                    action_id,
                    format_danger(&item.danger)
                ),
            );
            return;
        }

        let result = self.runtime.execute_action(&item.action_id, &args, false);
        self.record_submit(result);
    }

    fn preview_or_confirm(
        &mut self,
        action_id: &str,
        args: Vec<String>,
        title: String,
        fallback_message: String,
    ) {
        match self.runtime.preview_action(action_id, &args) {
            Ok(preview) => {
                self.ui_state.pending_preview = Some(PreviewDialog {
                    action_id: action_id.to_string(),
                    args,
                    title,
                    preview,
                    understood: false,
                });
            }
            Err(error) => {
                self.ui_state.pending_confirmation = Some(ConfirmationDialog {
                    action_id: action_id.to_string(),
                    args,
                    title,
                    message: format!("{fallback_message}\nPreview unavailable: {error}"),
                });
            }
        }
    }

    fn execute_action_direct(&mut self, action_id: &str, args: Vec<String>, confirmed: bool) {
        let result = self.runtime.execute_action(action_id, &args, confirmed);
        if result.is_ok() && action_id == "commit.create" {
            self.pending_commit_clear_after_version = Some(self.runtime.state().snapshot.version);
        }
        self.record_submit(result);
    }

    fn sync_local_ui_from_runtime(&mut self, state: &RuntimeAdapterState) {
        if let Some(root) = state.snapshot.repo.as_ref().map(|repo| repo.root.clone())
            && self.last_synced_repo_root.as_deref() != Some(root.as_str())
        {
            self.repo_path_input = root.clone();
            self.last_synced_repo_root = Some(root);
            if self.ui_error.as_deref() == Some("Enter a repository path before opening.") {
                self.ui_error = None;
            }
        }

        if let Some(version) = self.pending_commit_clear_after_version
            && !state.busy
            && state.last_error.is_none()
            && state.snapshot.version > version
        {
            self.commit_message.clear();
            self.commit_description.clear();
            self.pending_commit_clear_after_version = None;
        } else if !state.busy && state.last_error.is_some() {
            self.pending_commit_clear_after_version = None;
        }

        if !self.ui_state.active_panel.is_visible(
            self.ui_state.layout.advanced_mode,
            self.ui_state.layout.developer_mode,
        ) {
            self.ui_state.active_panel = PanelId::Status;
            self.ui_state.selected_sidebar_item = PanelId::Status;
        }
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
            let dark = ui.visuals().dark_mode;
            let repo = state.snapshot.repo.as_ref();
            let has_repo = repo.is_some();

            ui.set_min_height(design_tokens::TOPBAR_HEIGHT);
            ui.spacing_mut().item_spacing = egui::vec2(design_tokens::SPACING_SM, 4.0);
            egui::Frame::new()
                .fill(design_tokens::surface(dark))
                .inner_margin(egui::Margin::symmetric(12, 6))
                .show(ui, |ui| {
                    ui.horizontal(|ui| {
                        if action_button(ui, "Sidebar", ActionButtonKind::Ghost, true, None)
                            .clicked()
                        {
                            self.ui_state.layout.left_sidebar_open =
                                !self.ui_state.layout.left_sidebar_open;
                        }
                        if action_button(ui, "Inspector", ActionButtonKind::Ghost, true, None)
                            .clicked()
                        {
                            self.ui_state.layout.right_inspector_open =
                                !self.ui_state.layout.right_inspector_open;
                        }

                        ui.separator();
                        ui.vertical(|ui| {
                            ui.horizontal(|ui| {
                                ui.label(
                                    RichText::new("BranchForge")
                                        .size(design_tokens::FONT_SIZE_TITLE)
                                        .strong(),
                                );
                                if let Some(repo) = repo {
                                    ui.label(
                                        RichText::new(repo_display_name(&repo.root))
                                            .size(design_tokens::FONT_SIZE_LG)
                                            .strong(),
                                    );
                                }
                            });
                            if let Some(repo) = repo {
                                ui.label(
                                    RichText::new(truncate_middle(&repo.root, 46))
                                        .size(design_tokens::FONT_SIZE_SM)
                                        .color(design_tokens::text_muted(dark)),
                                )
                                .on_hover_text(repo.root.as_str());
                            } else {
                                let path_response = ui.add_enabled(
                                    !state.busy,
                                    egui::TextEdit::singleline(&mut self.repo_path_input)
                                        .desired_width(320.0)
                                        .hint_text("No repository opened"),
                                );
                                if path_response.lost_focus()
                                    && ui.input(|input| input.key_pressed(egui::Key::Enter))
                                {
                                    self.open_repo_from_input();
                                }
                            }
                        });

                        ui.separator();
                        ui.vertical(|ui| {
                            ui.horizontal_wrapped(|ui| {
                                if let Some(repo) = repo {
                                    let branch_text = branch_badge_label(&state.snapshot)
                                        .unwrap_or_else(|| {
                                            repo.head
                                                .clone()
                                                .unwrap_or_else(|| "detached HEAD".to_string())
                                        });
                                    let branch_color = if repo.head.is_some() {
                                        design_tokens::accent_soft(dark)
                                    } else {
                                        design_tokens::warning(dark)
                                    };
                                    ui.label(
                                        RichText::new(branch_text)
                                            .monospace()
                                            .background_color(branch_color)
                                            .color(design_tokens::text(dark)),
                                    );
                                    ui.label(
                                        RichText::new(dirty_summary(&state.snapshot))
                                            .color(design_tokens::text_muted(dark)),
                                    );
                                } else {
                                    ui.label(
                                        RichText::new("No repository opened")
                                            .color(design_tokens::text_muted(dark)),
                                    );
                                }
                            });
                            if state.busy {
                                ui.horizontal(|ui| {
                                    ui.spinner();
                                    ui.weak(
                                        state
                                            .current_operation
                                            .as_deref()
                                            .unwrap_or("Operation running"),
                                    );
                                });
                            }
                        });

                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                            let mut dark_mode = self.ui_state.layout.dark_mode;
                            if ui.checkbox(&mut dark_mode, "Dark").changed() {
                                self.ui_state.layout.dark_mode = dark_mode;
                                if dark_mode {
                                    ctx.set_visuals(egui::Visuals::dark());
                                } else {
                                    ctx.set_visuals(egui::Visuals::light());
                                }
                            }
                            ui.checkbox(&mut self.ui_state.layout.developer_mode, "Developer");
                            ui.checkbox(&mut self.ui_state.layout.advanced_mode, "Advanced");
                            if action_button(ui, "Palette", ActionButtonKind::Ghost, true, None)
                                .clicked()
                            {
                                self.ui_state.command_palette.open = true;
                            }
                            if action_button(
                                ui,
                                "Push",
                                ActionButtonKind::Secondary,
                                has_repo && !state.busy,
                                Some("Push current branch"),
                            )
                            .clicked()
                            {
                                self.preview_or_confirm(
                                    "remote.push",
                                    Vec::new(),
                                    "Preview push".to_string(),
                                    "Push current branch to upstream?".to_string(),
                                );
                            }
                            if action_button(
                                ui,
                                "Pull",
                                ActionButtonKind::Secondary,
                                has_repo && !state.busy,
                                Some("Pull current branch"),
                            )
                            .clicked()
                            {
                                self.preview_or_confirm(
                                    "remote.pull",
                                    Vec::new(),
                                    "Preview pull".to_string(),
                                    "Pull current branch from upstream?".to_string(),
                                );
                            }
                            if action_button(
                                ui,
                                "Fetch",
                                ActionButtonKind::Secondary,
                                has_repo && !state.busy,
                                Some("Fetch all remotes"),
                            )
                            .clicked()
                            {
                                self.execute_action_direct("remote.fetch_all", Vec::new(), false);
                            }
                            if action_button(
                                ui,
                                "Refresh",
                                ActionButtonKind::Secondary,
                                !state.busy,
                                None,
                            )
                            .clicked()
                            {
                                let result = self.runtime.refresh();
                                self.record_submit(result);
                            }
                            if action_button(
                                ui,
                                if has_repo { "Open" } else { "Open Repository" },
                                ActionButtonKind::Secondary,
                                !state.busy,
                                None,
                            )
                            .clicked()
                            {
                                if has_repo {
                                    self.open_repo_dialog();
                                } else {
                                    self.open_repo_from_input();
                                }
                            }
                            if action_button(
                                ui,
                                "Browse",
                                ActionButtonKind::Secondary,
                                !state.busy,
                                None,
                            )
                            .clicked()
                            {
                                self.open_repo_dialog();
                            }
                        });
                    });
                });
        });
    }

    fn render_sidebar(&mut self, root_ui: &mut egui::Ui) {
        if !self.ui_state.layout.left_sidebar_open {
            return;
        }

        egui::Panel::left("branchforge.sidebar")
            .resizable(true)
            .default_size(design_tokens::SIDEBAR_WIDTH)
            .size_range(190.0..=260.0)
            .show_inside(root_ui, |ui| {
                let dark = ui.visuals().dark_mode;
                ui.spacing_mut().item_spacing = egui::vec2(4.0, 4.0);
                ui.vertical(|ui| {
                    for &(section, panels) in sidebar_sections() {
                        let visible = panels.iter().any(|panel| {
                            panel.is_visible(
                                self.ui_state.layout.advanced_mode,
                                self.ui_state.layout.developer_mode,
                            )
                        });
                        if !visible {
                            continue;
                        }
                        ui.add_space(design_tokens::SPACING_SM);
                        ui.label(
                            RichText::new(section)
                                .size(design_tokens::FONT_SIZE_XS)
                                .color(design_tokens::text_subtle(dark))
                                .strong(),
                        );
                        for panel in panels {
                            if !panel.is_visible(
                                self.ui_state.layout.advanced_mode,
                                self.ui_state.layout.developer_mode,
                            ) {
                                continue;
                            }
                            self.render_sidebar_item(ui, *panel);
                        }
                    }
                });
            });
    }

    fn render_sidebar_item(&mut self, ui: &mut egui::Ui, panel: PanelId) {
        let dark = ui.visuals().dark_mode;
        let active = self.ui_state.active_panel == panel;
        ui.horizontal(|ui| {
            ui.set_height(design_tokens::ROW_HEIGHT_MD);
            if active {
                ui.colored_label(design_tokens::accent(dark), "|");
            } else {
                ui.add_space(6.0);
            }
            let response = ui.selectable_label(
                active,
                RichText::new(panel.label()).size(design_tokens::FONT_SIZE_MD),
            );
            if response.clicked() {
                self.activate_panel(panel);
            }
            if let Some(badge) = sidebar_badge(panel) {
                ui.label(
                    RichText::new(badge)
                        .size(design_tokens::FONT_SIZE_XS)
                        .color(match panel.panel_status() {
                            PanelStatus::Advanced | PanelStatus::DeveloperPreview => {
                                design_tokens::warning(dark)
                            }
                            PanelStatus::Developer => design_tokens::accent(dark),
                            PanelStatus::Preview => design_tokens::text_subtle(dark),
                            PanelStatus::Core => design_tokens::text_muted(dark),
                        }),
                );
            }
        });
    }

    fn render_inspector(&mut self, root_ui: &mut egui::Ui, state: &RuntimeAdapterState) {
        if !self.ui_state.layout.right_inspector_open {
            return;
        }

        egui::Panel::right("branchforge.inspector")
            .resizable(true)
            .default_size(design_tokens::INSPECTOR_WIDTH)
            .size_range(240.0..=360.0)
            .show_inside(root_ui, |ui| {
                egui::ScrollArea::vertical().show(ui, |ui| {
                    let dark = ui.visuals().dark_mode;
                    ui.label(
                        RichText::new("Inspector")
                            .size(design_tokens::FONT_SIZE_LG)
                            .strong(),
                    );
                    ui.separator();
                    if let Some(repo) = state.snapshot.repo.as_ref() {
                        ui.label(RichText::new(repo_display_name(&repo.root)).strong());
                        ui.label(
                            RichText::new(truncate_middle(&repo.root, 34))
                                .color(design_tokens::text_muted(dark)),
                        )
                        .on_hover_text(repo.root.as_str());
                    } else {
                        ui.weak("No repository opened");
                    }
                    ui.separator();
                    match self.ui_state.active_panel {
                        PanelId::Status => {
                            ui.label(RichText::new("Status").strong());
                            ui.label(format!("Staged: {}", state.snapshot.status.staged.len()));
                            ui.label(format!(
                                "Unstaged: {}",
                                state.snapshot.status.unstaged.len()
                            ));
                            ui.label(format!(
                                "Untracked: {}",
                                state.snapshot.status.untracked.len()
                            ));
                            ui.separator();
                            if let Some(file) = self.selected_file(&state.snapshot) {
                                ui.label(RichText::new("Selected file").strong());
                                ui.label(file);
                                self.render_selected_file_actions(ui, &state.snapshot, state.busy);
                            } else {
                                render_empty_state(
                                    ui,
                                    "No file selected",
                                    "Select a changed file to stage it or open its diff.",
                                );
                            }
                        }
                        PanelId::History => {
                            ui.label(RichText::new("Commit").strong());
                            if let Some(commit) =
                                state.snapshot.selection.selected_commit_oid.as_deref()
                            {
                                if let Some(details) = state.snapshot.commit_cache.get(commit) {
                                    ui.label(RichText::new(short_oid(commit)).monospace());
                                    ui.label(details.message.lines().next().unwrap_or(""));
                                    ui.label(format!("Author: {}", details.author));
                                    ui.label(format!("Date: {}", details.time));
                                    if !details.refs.is_empty() {
                                        ui.label(format!("Refs: {}", details.refs.join(", ")));
                                    }
                                    ui.horizontal(|ui| {
                                        if action_button(
                                            ui,
                                            "Copy hash",
                                            ActionButtonKind::Secondary,
                                            true,
                                            None,
                                        )
                                        .clicked()
                                        {
                                            ui.ctx().copy_text(details.oid.clone());
                                        }
                                        if action_button(
                                            ui,
                                            "Show diff",
                                            ActionButtonKind::Secondary,
                                            true,
                                            None,
                                        )
                                        .clicked()
                                        {
                                            self.activate_panel(PanelId::Diff);
                                        }
                                    });
                                } else {
                                    ui.label(RichText::new(short_oid(commit)).monospace());
                                }
                            } else {
                                render_empty_state(
                                    ui,
                                    "No commit selected",
                                    "Select a commit in History to inspect it here.",
                                );
                            }
                        }
                        PanelId::Diff => {
                            ui.label(RichText::new("Diff").strong());
                            ui.label(
                                state
                                    .snapshot
                                    .diff
                                    .source
                                    .as_ref()
                                    .map(format_diff_source)
                                    .unwrap_or_else(|| "No diff selected".to_string()),
                            );
                            if let Some(hunk_index) = self.ui_state.diff_view_state.selected_hunk {
                                ui.label(format!("Selected hunk: {}", hunk_index + 1));
                            } else {
                                ui.weak("No hunk selected");
                            }
                            ui.label(format!(
                                "Selected lines: {}",
                                self.ui_state.diff_view_state.selected_changed_lines.len()
                            ));
                            ui.label(format!("Chunks: {}", state.snapshot.diff.chunks.len()));
                            ui.label(format!("Hunks: {}", state.snapshot.diff.hunks.len()));
                            if state.snapshot.diff.loading {
                                ui.weak("Loading");
                            }
                        }
                        PanelId::Branches => {
                            ui.label(RichText::new("Branch").strong());
                            if let Some(branch) =
                                state.snapshot.selection.selected_branch.as_deref()
                            {
                                let branch_info = state
                                    .snapshot
                                    .branches
                                    .branches
                                    .iter()
                                    .find(|item| item.name == branch);
                                ui.label(RichText::new(branch).monospace());
                                if branch_info.is_some_and(|item| item.is_current) {
                                    ui.label("Current branch");
                                }
                                ui.label(format!(
                                    "Upstream: {}",
                                    branch_info
                                        .and_then(|item| item.upstream.as_deref())
                                        .unwrap_or("<none>")
                                ));
                                self.render_selected_branch_actions(
                                    ui,
                                    &state.snapshot,
                                    branch,
                                    state.busy,
                                );
                            } else {
                                render_empty_state(
                                    ui,
                                    "No branch selected",
                                    "Select a branch to see checkout and maintenance actions.",
                                );
                            }
                        }
                        PanelId::Tags => {
                            ui.label(RichText::new("Tag").strong());
                            ui.weak("Select tag actions in the Tags panel.");
                        }
                        PanelId::Remotes => {
                            ui.label(RichText::new("Remote").strong());
                            ui.label(format!("Remotes: {}", state.snapshot.remotes.remotes.len()));
                            ui.label(format!(
                                "SSH agent: {}",
                                bool_word(state.snapshot.remotes.auth.ssh_agent_available)
                            ));
                            ui.label(format!(
                                "Credential helper: {}",
                                bool_word(state.snapshot.remotes.auth.https_helper_configured)
                            ));
                        }
                        _ => {
                            ui.label(format!("Panel: {}", self.ui_state.active_panel.label()));
                            ui.weak(
                                "Context-specific details will appear as this workflow stabilizes.",
                            );
                        }
                    }
                    if self.ui_state.layout.developer_mode
                        && let Some(capabilities) = state.snapshot.repo_capabilities.as_ref()
                    {
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
                    if self.ui_state.layout.developer_mode {
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
            ui.set_min_height(design_tokens::BOTTOMBAR_HEIGHT);
            ui.horizontal(|ui| {
                let message = bottom_status_message(state, self.ui_error.as_deref());
                if message.is_error {
                    ui.colored_label(
                        design_tokens::danger(ui.visuals().dark_mode),
                        format!("Last error: {}", message.text),
                    );
                    if action_button(ui, "Clear", ActionButtonKind::Ghost, true, None).clicked() {
                        self.ui_error = None;
                    }
                } else {
                    ui.label(format!(
                        "{} · {} · Jobs: {} · Journal: {}",
                        message.text,
                        bottom_repo_context_label(&state.snapshot),
                        if state.busy { 1 } else { 0 },
                        state.snapshot.journal.entries.len()
                    ));
                }
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
                PanelId::Remotes => self.render_remotes_panel(ui, state),
                PanelId::Workspaces => self.render_workspaces_panel(ui, state),
                PanelId::PullRequests => self.render_pull_requests_panel(ui, state),
                PanelId::BranchStacks => self.render_branch_stacks_panel(ui, state),
                PanelId::Stash => self.render_stash_panel(ui, state),
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
        render_panel_header(ui, "Status", &dirty_summary(snapshot));
        if let Some(conflict_state) = snapshot
            .repo
            .as_ref()
            .and_then(|repo| repo.conflict_state.as_ref())
        {
            ui.colored_label(
                design_tokens::warning(ui.visuals().dark_mode),
                format!("Conflict: {}", format_conflict_state(conflict_state)),
            );
        }

        ui.horizontal(|ui| {
            let stage_all_paths = stageable_paths(snapshot);
            if action_button(
                ui,
                "Stage all",
                ActionButtonKind::Secondary,
                !stage_all_paths.is_empty() && !state.busy,
                Some("No unstaged or untracked files"),
            )
            .clicked()
            {
                self.execute_action_direct("index.stage_paths", stage_all_paths, false);
            }
            if action_button(
                ui,
                "Unstage all",
                ActionButtonKind::Secondary,
                !snapshot.status.staged.is_empty() && !state.busy,
                Some("No staged files"),
            )
            .clicked()
            {
                self.execute_action_direct(
                    "index.unstage_paths",
                    snapshot.status.staged.clone(),
                    false,
                );
            }
            if action_button(
                ui,
                "Refresh",
                ActionButtonKind::Secondary,
                !state.busy,
                None,
            )
            .clicked()
            {
                self.execute_action_direct("status.refresh", Vec::new(), false);
            }
        });

        ui.add_space(design_tokens::SPACING_MD);
        ui.label(RichText::new("Changes").strong());
        self.render_file_group(
            ui,
            "Staged",
            "A",
            &snapshot.status.staged,
            &snapshot.selection.selected_paths,
            state.busy,
        );
        self.render_file_group(
            ui,
            "Unstaged",
            "M",
            &snapshot.status.unstaged,
            &snapshot.selection.selected_paths,
            state.busy,
        );
        self.render_file_group(
            ui,
            "Untracked",
            "?",
            &snapshot.status.untracked,
            &snapshot.selection.selected_paths,
            state.busy,
        );

        if let Some(file) = self.selected_file(snapshot) {
            ui.add_space(design_tokens::SPACING_SM);
            ui.horizontal_wrapped(|ui| {
                ui.label(RichText::new("Selected file").strong());
                ui.label(file);
                self.render_selected_file_actions(ui, snapshot, state.busy);
            });
        }

        ui.add_space(design_tokens::SPACING_MD);
        self.render_commit_block(ui, snapshot, state.busy);
    }

    fn render_file_group(
        &mut self,
        ui: &mut egui::Ui,
        title: &str,
        badge: &str,
        files: &[String],
        selected_paths: &[String],
        busy: bool,
    ) {
        egui::Frame::group(ui.style())
            .fill(design_tokens::surface(ui.visuals().dark_mode))
            .show(ui, |ui| {
                ui.horizontal(|ui| {
                    ui.label(RichText::new(format!("{title} ({})", files.len())).strong());
                });
                if files.is_empty() {
                    ui.weak("empty");
                    return;
                }
                for file in files {
                    let selected = selected_paths.iter().any(|path| path == file);
                    let response = ui.add_enabled(
                        !busy,
                        egui::Button::selectable(selected, format!("{badge} {file}"))
                            .min_size(egui::vec2(0.0, design_tokens::ROW_HEIGHT_SM)),
                    );
                    if response.clicked() {
                        let result = self.runtime.select_file(file);
                        self.record_submit(result);
                    }
                }
            });
        ui.add_space(design_tokens::SPACING_SM);
    }

    fn render_selected_file_actions(
        &mut self,
        ui: &mut egui::Ui,
        snapshot: &StoreSnapshot,
        busy: bool,
    ) {
        let Some(file) = self.selected_file(snapshot).map(str::to_string) else {
            return;
        };
        let group = selected_file_group(snapshot, &file);
        let can_stage = matches!(
            group,
            Some(StatusFileGroup::Unstaged | StatusFileGroup::Untracked)
        );
        let can_unstage = matches!(group, Some(StatusFileGroup::Staged));
        if action_button(
            ui,
            "Stage",
            ActionButtonKind::Secondary,
            can_stage && !busy,
            Some("Only unstaged or untracked files can be staged"),
        )
        .clicked()
        {
            self.execute_action_direct("index.stage_paths", vec![file.clone()], false);
        }
        if action_button(
            ui,
            "Unstage",
            ActionButtonKind::Secondary,
            can_unstage && !busy,
            Some("Only staged files can be unstaged"),
        )
        .clicked()
        {
            self.execute_action_direct("index.unstage_paths", vec![file.clone()], false);
        }
        if action_button(ui, "Open Diff", ActionButtonKind::Secondary, !busy, None).clicked() {
            let result = self.runtime.select_file(&file);
            self.record_submit(result);
            self.activate_panel(PanelId::Diff);
        }
        if action_button(
            ui,
            "Discard",
            ActionButtonKind::Danger,
            (can_stage || can_unstage) && !busy,
            Some("Discard writes to the worktree and requires confirmation"),
        )
        .clicked()
        {
            self.preview_or_confirm(
                "file.discard",
                vec![file.clone()],
                "Preview discard file".to_string(),
                format!("Discard changes in {file}?"),
            );
        }
        if action_button(
            ui,
            "Reveal",
            ActionButtonKind::Ghost,
            snapshot.repo.is_some(),
            Some("Reveal this file in the system file manager"),
        )
        .clicked()
            && let Some(repo) = snapshot.repo.as_ref()
            && let Err(error) = reveal_repo_path(&repo.root, &file)
        {
            self.ui_error = Some(error);
        }
        if action_button(ui, "Copy path", ActionButtonKind::Ghost, true, None).clicked() {
            ui.ctx().copy_text(file);
        }
    }

    fn render_commit_block(&mut self, ui: &mut egui::Ui, snapshot: &StoreSnapshot, busy: bool) {
        egui::Frame::group(ui.style())
            .fill(design_tokens::surface(ui.visuals().dark_mode))
            .show(ui, |ui| {
                ui.label(RichText::new("Commit").strong());
                ui.label("Summary");
                ui.add_sized(
                    [f32::INFINITY, design_tokens::INPUT_HEIGHT_MD],
                    egui::TextEdit::singleline(&mut self.commit_message)
                        .hint_text("Commit summary"),
                );
                ui.label("Description");
                ui.add(
                    egui::TextEdit::multiline(&mut self.commit_description)
                        .desired_rows(3)
                        .desired_width(f32::INFINITY)
                        .hint_text("Optional description"),
                );
                let staged_count = snapshot.status.staged.len();
                let reason = commit_disabled_reason(snapshot, &self.commit_message, busy);
                let can_commit = reason.is_none();
                if action_button(
                    ui,
                    &commit_button_label(staged_count),
                    ActionButtonKind::Primary,
                    can_commit,
                    reason,
                )
                .clicked()
                {
                    let mut message = self.commit_message.trim().to_string();
                    let description = self.commit_description.trim();
                    if !description.is_empty() {
                        message.push_str("\n\n");
                        message.push_str(description);
                    }
                    self.execute_action_direct("commit.create", vec![message], false);
                }
            });
    }

    fn render_selected_branch_actions(
        &mut self,
        ui: &mut egui::Ui,
        snapshot: &StoreSnapshot,
        branch: &str,
        busy: bool,
    ) {
        let selected_is_current = snapshot
            .branches
            .branches
            .iter()
            .any(|item| item.name == branch && item.is_current);
        if action_button(
            ui,
            "Checkout",
            ActionButtonKind::Primary,
            !busy && !selected_is_current,
            Some("Current branch is already checked out"),
        )
        .clicked()
        {
            if status_is_dirty(snapshot) {
                self.ui_state.pending_confirmation = Some(ConfirmationDialog {
                    action_id: "branch.checkout".to_string(),
                    args: vec![branch.to_string()],
                    title: "You have local changes".to_string(),
                    message: "Switching branches may affect your working tree. Continue anyway?"
                        .to_string(),
                });
            } else {
                self.execute_action_direct("branch.checkout", vec![branch.to_string()], false);
            }
        }

        let rename_target = self.branch_name_input.trim();
        if action_button(
            ui,
            "Rename",
            ActionButtonKind::Secondary,
            !busy
                && !selected_is_current
                && branch_name_input_is_valid(rename_target)
                && rename_target != branch,
            Some("Enter a valid new branch name in the branch input"),
        )
        .clicked()
        {
            self.preview_or_confirm(
                "branch.rename",
                vec![branch.to_string(), rename_target.to_string()],
                "Preview branch rename".to_string(),
                format!("Rename branch {branch} to {rename_target}?"),
            );
        }

        if action_button(
            ui,
            "Delete",
            ActionButtonKind::Danger,
            !busy && !selected_is_current,
            Some("Current branch cannot be deleted"),
        )
        .clicked()
        {
            self.preview_or_confirm(
                "branch.delete",
                vec![branch.to_string()],
                "Preview branch delete".to_string(),
                format!("Delete branch {branch}?"),
            );
        }

        if action_button(ui, "Copy branch name", ActionButtonKind::Ghost, true, None).clicked() {
            ui.ctx().copy_text(branch.to_string());
        }
    }

    fn render_history_panel(&mut self, ui: &mut egui::Ui, state: &RuntimeAdapterState) {
        let snapshot = &state.snapshot;
        render_panel_header(
            ui,
            "History",
            &format!("{} commits loaded", snapshot.history.commits.len()),
        );
        ui.horizontal(|ui| {
            if action_button(
                ui,
                "Refresh",
                ActionButtonKind::Secondary,
                !state.busy,
                None,
            )
            .clicked()
            {
                self.activate_panel(PanelId::History);
            }
            if action_button(
                ui,
                "Load more",
                ActionButtonKind::Secondary,
                snapshot.history.next_cursor.is_some() && !state.busy,
                Some("No more commits available"),
            )
            .clicked()
            {
                self.execute_action_direct("history.load_more", Vec::new(), false);
            }
        });
        ui.horizontal(|ui| {
            ui.add(
                egui::TextEdit::singleline(&mut self.ui_state.graph_view_state.search)
                    .desired_width(260.0)
                    .hint_text("Search commits"),
            );
            if ui
                .add_enabled(!state.busy, egui::Button::new("Search"))
                .clicked()
            {
                self.execute_action_direct(
                    "history.search",
                    vec![
                        "0".to_string(),
                        "20".to_string(),
                        String::new(),
                        self.ui_state.graph_view_state.search.trim().to_string(),
                    ],
                    false,
                );
            }
            if ui
                .add_enabled(!state.busy, egui::Button::new("Clear filter"))
                .clicked()
            {
                self.ui_state.graph_view_state.search.clear();
                self.execute_action_direct("history.clear_filter", Vec::new(), false);
            }
        });
        if snapshot.history.loading {
            render_loading_state(ui, "Loading history...");
        }
        if let Some(error) = snapshot.history.error.as_deref() {
            render_error_state(ui, "Could not load history.", error);
        }
        if snapshot.history.commits.is_empty() {
            render_empty_state(
                ui,
                "No commits",
                "History will appear after commits are loaded.",
            );
            return;
        }
        let graph = if state.graph_rows.is_empty() {
            build_visible_graph(snapshot)
        } else {
            state.graph_rows.clone()
        };

        for commit in &graph {
            let selected =
                snapshot.selection.selected_commit_oid.as_deref() == Some(commit.oid.as_str());
            let row_fill = if selected {
                design_tokens::surface_active(ui.visuals().dark_mode)
            } else {
                design_tokens::surface(ui.visuals().dark_mode)
            };
            egui::Frame::group(ui.style())
                .fill(row_fill)
                .show(ui, |ui| {
                    let response = ui.selectable_label(
                        selected,
                        RichText::new(format!("{}  {}", short_oid(&commit.oid), commit.summary))
                            .size(design_tokens::FONT_SIZE_MD)
                            .strong(),
                    );
                    if response.clicked() {
                        self.ui_state.graph_view_state.selected_row = Some(commit.row);
                        let result = self.runtime.select_commit(&commit.oid);
                        self.record_submit(result);
                    }
                    response.context_menu(|ui| {
                        if ui.button("Copy hash").clicked() {
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
                    ui.horizontal_wrapped(|ui| {
                        ui.label(
                            RichText::new(short_oid(&commit.oid))
                                .monospace()
                                .color(design_tokens::text_muted(ui.visuals().dark_mode)),
                        );
                        if !commit.refs.is_empty() {
                            for label in &commit.refs {
                                ui.label(ref_label_text(label));
                            }
                        }
                        ui.label(
                            RichText::new(commit.author.as_str())
                                .color(design_tokens::text_muted(ui.visuals().dark_mode)),
                        );
                        ui.label(
                            RichText::new(commit.time.as_str())
                                .color(design_tokens::text_muted(ui.visuals().dark_mode)),
                        );
                    });
                });
            ui.add_space(design_tokens::SPACING_XS);
        }

        if let Some(commit) = snapshot.selection.selected_commit_oid.as_deref() {
            ui.separator();
            ui.label(RichText::new("Selected commit").strong());
            if let Some(details) = snapshot.commit_cache.get(commit) {
                ui.label(format!(
                    "{}  {}",
                    short_oid(&details.oid),
                    details.message.lines().next().unwrap_or("")
                ));
                ui.horizontal(|ui| {
                    if action_button(ui, "Copy hash", ActionButtonKind::Secondary, true, None)
                        .clicked()
                    {
                        ui.ctx().copy_text(details.oid.clone());
                    }
                    if action_button(ui, "Open diff", ActionButtonKind::Secondary, true, None)
                        .clicked()
                    {
                        self.activate_panel(PanelId::Diff);
                    }
                });
            } else {
                ui.label(format!("Hash: {commit}"));
                if action_button(ui, "Copy hash", ActionButtonKind::Secondary, true, None).clicked()
                {
                    ui.ctx().copy_text(commit.to_string());
                }
            }
        }
    }

    fn render_diff_panel(&mut self, ui: &mut egui::Ui, state: &RuntimeAdapterState) {
        let snapshot = &state.snapshot;
        render_panel_header(ui, "Diff", &diff_header_summary(snapshot));
        ui.horizontal(|ui| {
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

        if snapshot.diff.loading {
            render_loading_state(ui, "Loading diff...");
        }
        if let Some(error) = snapshot.diff.error.as_deref() {
            render_error_state(ui, "Could not load diff.", error);
        }
        if snapshot.diff.hunks.is_empty() {
            if let Some(content) = snapshot.diff.content.as_deref() {
                render_diff_text(ui, content);
            } else {
                render_empty_state(
                    ui,
                    "No diff selected",
                    "Select a file in Status or a commit in History.",
                );
            }
            return;
        }

        self.render_diff_actions(ui, snapshot, state.busy);
        ui.separator();

        for (visible_index, hunk) in snapshot.diff.hunks.iter().enumerate() {
            let selected = self.ui_state.diff_view_state.selected_hunk == Some(visible_index);
            let label = format!("{}  {}", hunk.file_path, hunk.header);
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
        if snapshot
            .diff
            .descriptor
            .as_ref()
            .is_some_and(|descriptor| descriptor.truncated)
        {
            ui.separator();
            action_button(
                ui,
                "Load more diff",
                ActionButtonKind::Secondary,
                false,
                Some("Large diff support is limited to the first rendered portion for now"),
            );
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
                        if action_button(
                            ui,
                            "Stage hunk",
                            ActionButtonKind::Secondary,
                            !busy,
                            Some("Open a worktree diff and select a hunk first"),
                        )
                        .clicked()
                        {
                            self.execute_action_direct(
                                "index.stage_hunk",
                                vec![hunk.file_path.clone(), hunk.hunk_index.to_string()],
                                false,
                            );
                        }
                        if action_button(
                            ui,
                            "Stage selected lines",
                            ActionButtonKind::Secondary,
                            !busy && !selected_lines.is_empty(),
                            Some("Select changed lines inside the current hunk"),
                        )
                        .clicked()
                        {
                            let mut args =
                                vec![hunk.file_path.clone(), hunk.hunk_index.to_string()];
                            args.extend(selected_lines.iter().map(usize::to_string));
                            self.execute_action_direct("index.stage_lines", args, false);
                        }
                        if action_button(
                            ui,
                            "Discard hunk",
                            ActionButtonKind::Danger,
                            !busy,
                            Some("Discarding a hunk writes to the worktree and requires confirmation"),
                        )
                        .clicked()
                        {
                            self.preview_or_confirm(
                                "file.discard_hunk",
                                vec![hunk.file_path.clone(), hunk.hunk_index.to_string()],
                                "Preview discard hunk".to_string(),
                                "Discarding a hunk writes to the worktree.".to_string(),
                            );
                        }
                        if action_button(
                            ui,
                            "Discard selected lines",
                            ActionButtonKind::Danger,
                            !busy && !selected_lines.is_empty(),
                            Some("Select changed lines before discarding selected lines"),
                        )
                        .clicked()
                        {
                            let mut args =
                                vec![hunk.file_path.clone(), hunk.hunk_index.to_string()];
                            args.extend(selected_lines.iter().map(usize::to_string));
                            self.preview_or_confirm(
                                "file.discard_lines",
                                args,
                                "Preview discard selected lines".to_string(),
                                "Discarding selected lines writes to the worktree.".to_string(),
                            );
                        }
                    }
                    Some(DiffSource::Index { .. }) => {
                        if action_button(
                            ui,
                            "Unstage hunk",
                            ActionButtonKind::Secondary,
                            !busy,
                            Some("Open an index diff and select a hunk first"),
                        )
                        .clicked()
                        {
                            self.execute_action_direct(
                                "index.unstage_hunk",
                                vec![hunk.file_path.clone(), hunk.hunk_index.to_string()],
                                false,
                            );
                        }
                        if action_button(
                            ui,
                            "Unstage selected lines",
                            ActionButtonKind::Secondary,
                            !busy && !selected_lines.is_empty(),
                            Some("Select changed lines inside the current hunk"),
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
                let source = snapshot.diff.source.as_ref();
                let worktree = matches!(source, Some(DiffSource::Worktree { .. }));
                let index = matches!(source, Some(DiffSource::Index { .. }));
                action_button(
                    ui,
                    "Stage hunk",
                    ActionButtonKind::Secondary,
                    false,
                    Some("Open a worktree diff and select a hunk first"),
                );
                action_button(
                    ui,
                    "Unstage hunk",
                    ActionButtonKind::Secondary,
                    false,
                    Some("Open an index diff and select a hunk first"),
                );
                action_button(
                    ui,
                    "Discard hunk",
                    ActionButtonKind::Danger,
                    false,
                    Some(if worktree || index {
                        "Select a hunk first"
                    } else {
                        "Open a mutable worktree diff first"
                    }),
                );
            }
        });
    }

    fn render_branches_panel(&mut self, ui: &mut egui::Ui, state: &RuntimeAdapterState) {
        let snapshot = &state.snapshot;
        render_panel_header(
            ui,
            "Branches",
            &format!("{} local branches", snapshot.branches.branches.len()),
        );
        ui.horizontal(|ui| {
            if action_button(
                ui,
                "Refresh refs",
                ActionButtonKind::Secondary,
                !state.busy,
                None,
            )
            .clicked()
            {
                self.execute_action_direct("refs.refresh", Vec::new(), false);
            }
        });

        ui.add_space(design_tokens::SPACING_MD);
        ui.label(RichText::new("Create branch").strong());
        ui.horizontal(|ui| {
            ui.add(
                egui::TextEdit::singleline(&mut self.branch_name_input)
                    .desired_width(220.0)
                    .hint_text("New branch name"),
            );
            let branch_name_valid = branch_name_input_is_valid(self.branch_name_input.trim());
            if action_button(
                ui,
                "Create",
                ActionButtonKind::Primary,
                branch_name_valid && !state.busy,
                Some("Enter a valid Git branch name"),
            )
            .clicked()
            {
                self.execute_action_direct(
                    "branch.create",
                    vec![self.branch_name_input.trim().to_string()],
                    false,
                );
            }
            if action_button(
                ui,
                "Create & Checkout",
                ActionButtonKind::Secondary,
                branch_name_valid && !state.busy,
                Some("Create a branch and switch to it"),
            )
            .clicked()
            {
                if status_is_dirty(snapshot) {
                    self.ui_state.pending_confirmation = Some(ConfirmationDialog {
                        action_id: "branch.create_checkout".to_string(),
                        args: vec![self.branch_name_input.trim().to_string()],
                        title: "You have local changes".to_string(),
                        message:
                            "Creating and checking out a branch may affect your working tree. Continue anyway?"
                                .to_string(),
                    });
                } else {
                    self.execute_action_direct(
                        "branch.create_checkout",
                        vec![self.branch_name_input.trim().to_string()],
                        false,
                    );
                }
            }
            if !self.branch_name_input.trim().is_empty() && !branch_name_valid {
                ui.colored_label(
                    design_tokens::warning(ui.visuals().dark_mode),
                    "Invalid branch name",
                );
            }
        });

        if let Some(branch) = snapshot.selection.selected_branch.as_deref() {
            ui.add_space(design_tokens::SPACING_SM);
            ui.horizontal_wrapped(|ui| {
                ui.label(RichText::new("Selected branch").strong());
                ui.label(RichText::new(branch).monospace());
                self.render_selected_branch_actions(ui, snapshot, branch, state.busy);
            });
        }

        ui.add_space(design_tokens::SPACING_MD);
        ui.label(RichText::new("Local branches").strong());
        if snapshot.branches.branches.is_empty() {
            render_empty_state(
                ui,
                "No local branches",
                "Refresh refs after opening a repository.",
            );
            return;
        }

        for branch in &snapshot.branches.branches {
            let selected =
                snapshot.selection.selected_branch.as_deref() == Some(branch.name.as_str());
            let fill = if selected {
                design_tokens::surface_active(ui.visuals().dark_mode)
            } else {
                design_tokens::surface(ui.visuals().dark_mode)
            };
            egui::Frame::group(ui.style()).fill(fill).show(ui, |ui| {
                let label = if branch.is_current {
                    format!("{}    current", branch.name)
                } else {
                    branch.name.clone()
                };
                if ui
                    .selectable_label(selected, RichText::new(label).monospace().strong())
                    .clicked()
                {
                    let result = self.runtime.select_branch(&branch.name);
                    self.record_submit(result);
                }
                ui.label(
                    RichText::new(format!(
                        "upstream: {}",
                        branch.upstream.as_deref().unwrap_or("<none>")
                    ))
                    .color(design_tokens::text_muted(ui.visuals().dark_mode)),
                );
            });
            ui.add_space(design_tokens::SPACING_XS);
        }
    }

    fn render_tags_panel(&mut self, ui: &mut egui::Ui, state: &RuntimeAdapterState) {
        render_panel_header(
            ui,
            "Tags",
            &format!("{} tags", state.snapshot.tags.tags.len()),
        );
        if state.snapshot.tags.tags.is_empty() {
            render_empty_state(ui, "No tags", "Repository tags will appear here.");
            return;
        }

        for tag in &state.snapshot.tags.tags {
            egui::Frame::group(ui.style()).show(ui, |ui| {
                ui.horizontal(|ui| {
                    ui.label(RichText::new(tag.name.as_str()).monospace().strong());
                    if action_button(
                        ui,
                        "Checkout",
                        ActionButtonKind::Secondary,
                        !state.busy,
                        Some("Checking out a tag will detach HEAD"),
                    )
                    .clicked()
                    {
                        self.ui_state.pending_confirmation = Some(ConfirmationDialog {
                            action_id: "tag.checkout".to_string(),
                            args: vec![tag.name.clone()],
                            title: "Checkout tag".to_string(),
                            message: "Checking out a tag will detach HEAD. Continue?".to_string(),
                        });
                    }
                    if action_button(
                        ui,
                        "Delete",
                        ActionButtonKind::Danger,
                        !state.busy,
                        Some("Deleting a tag requires confirmation"),
                    )
                    .clicked()
                    {
                        self.preview_or_confirm(
                            "tag.delete",
                            vec![tag.name.clone()],
                            "Preview tag delete".to_string(),
                            format!("Delete tag {}?", tag.name),
                        );
                    }
                });
            });
            ui.add_space(design_tokens::SPACING_XS);
        }
    }

    fn render_compare_panel(&mut self, ui: &mut egui::Ui, state: &RuntimeAdapterState) {
        let snapshot = &state.snapshot;
        render_panel_header(
            ui,
            "Compare",
            "Compare two refs to inspect commits and file changes.",
        );
        render_panel_notice(ui, PanelId::Compare);
        ui.label(RichText::new("Base ref").strong());
        ui.add(
            egui::TextEdit::singleline(&mut self.compare_base_input)
                .desired_width(260.0)
                .hint_text("Base ref"),
        );
        ui.label(RichText::new("Head ref").strong());
        ui.horizontal(|ui| {
            ui.add(
                egui::TextEdit::singleline(&mut self.compare_head_input)
                    .desired_width(260.0)
                    .hint_text("Head ref"),
            );
            if action_button(
                ui,
                "Compare",
                ActionButtonKind::Primary,
                !self.compare_base_input.trim().is_empty()
                    && !self.compare_head_input.trim().is_empty()
                    && !state.busy,
                Some("Both refs are required"),
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
        if snapshot.compare.base_ref.is_none() || snapshot.compare.head_ref.is_none() {
            render_empty_state(
                ui,
                "No comparison",
                "Enter a base ref and head ref to compare commits and files.",
            );
            return;
        }
        ui.label(format!(
            "Ahead / behind: +{} / -{}",
            snapshot.compare.ahead, snapshot.compare.behind
        ));
        if snapshot.compare.commits.is_empty() {
            ui.weak("No commits between refs.");
        } else {
            for commit in snapshot.compare.commits.iter().take(30) {
                ui.label(format!("{} {}", short_oid(&commit.oid), commit.summary));
            }
            if action_button(ui, "Open diff", ActionButtonKind::Secondary, true, None).clicked() {
                self.activate_panel(PanelId::Diff);
            }
        }
    }

    fn render_remotes_panel(&mut self, ui: &mut egui::Ui, state: &RuntimeAdapterState) {
        let snapshot = &state.snapshot;
        render_panel_header(
            ui,
            "Remotes",
            &format!("{} remotes", snapshot.remotes.remotes.len()),
        );
        ui.horizontal(|ui| {
            if action_button(
                ui,
                "Refresh",
                ActionButtonKind::Secondary,
                !state.busy,
                None,
            )
            .clicked()
            {
                self.execute_action_direct("remote.refresh", Vec::new(), false);
            }
            if action_button(ui, "Fetch", ActionButtonKind::Secondary, !state.busy, None).clicked()
            {
                self.execute_action_direct("remote.fetch_all", Vec::new(), false);
            }
            if action_button(ui, "Pull", ActionButtonKind::Secondary, !state.busy, None).clicked() {
                self.preview_or_confirm(
                    "remote.pull",
                    Vec::new(),
                    "Preview pull".to_string(),
                    "Pull current branch from upstream?".to_string(),
                );
            }
            if action_button(ui, "Push", ActionButtonKind::Secondary, !state.busy, None).clicked() {
                self.preview_or_confirm(
                    "remote.push",
                    Vec::new(),
                    "Preview push".to_string(),
                    "Push current branch to upstream?".to_string(),
                );
            }
        });
        ui.add_space(design_tokens::SPACING_MD);
        ui.label(RichText::new("Add remote").strong());
        ui.horizontal(|ui| {
            ui.add(
                egui::TextEdit::singleline(&mut self.remote_name_input)
                    .desired_width(120.0)
                    .hint_text("name"),
            );
            ui.add(
                egui::TextEdit::singleline(&mut self.remote_url_input)
                    .desired_width(360.0)
                    .hint_text("remote URL"),
            );
            if action_button(
                ui,
                "Add",
                ActionButtonKind::Secondary,
                !self.remote_name_input.trim().is_empty()
                    && !self.remote_url_input.trim().is_empty()
                    && !state.busy,
                Some("Remote name and URL are required"),
            )
            .clicked()
            {
                self.preview_or_confirm(
                    "remote.add",
                    vec![
                        self.remote_name_input.trim().to_string(),
                        self.remote_url_input.trim().to_string(),
                    ],
                    "Preview add remote".to_string(),
                    "Add this remote to the repository?".to_string(),
                );
            }
        });

        ui.add_space(design_tokens::SPACING_MD);
        ui.label(RichText::new("Remote list").strong());
        if snapshot.remotes.remotes.is_empty() {
            render_empty_state(ui, "No remotes", "Add a remote or refresh repository refs.");
        }
        for remote in &snapshot.remotes.remotes {
            egui::Frame::group(ui.style()).show(ui, |ui| {
                ui.label(RichText::new(remote.name.as_str()).strong());
                ui.label(format!(
                    "fetch: {}",
                    remote.fetch_url.as_deref().unwrap_or("<none>")
                ));
                ui.label(format!(
                    "push: {}",
                    remote.push_url.as_deref().unwrap_or("<none>")
                ));
                ui.horizontal_wrapped(|ui| {
                    if action_button(ui, "Fetch", ActionButtonKind::Secondary, !state.busy, None)
                        .clicked()
                    {
                        self.execute_action_direct(
                            "remote.fetch",
                            vec![remote.name.clone()],
                            false,
                        );
                    }
                    if action_button(ui, "Pull", ActionButtonKind::Secondary, !state.busy, None)
                        .clicked()
                    {
                        self.preview_or_confirm(
                            "remote.pull",
                            Vec::new(),
                            "Preview pull".to_string(),
                            "Pull current branch from upstream?".to_string(),
                        );
                    }
                    if action_button(ui, "Push", ActionButtonKind::Secondary, !state.busy, None)
                        .clicked()
                    {
                        self.preview_or_confirm(
                            "remote.push",
                            Vec::new(),
                            "Preview push".to_string(),
                            "Push current branch to upstream?".to_string(),
                        );
                    }
                    let current_branch = snapshot
                        .repo
                        .as_ref()
                        .and_then(|repo| repo.head.clone())
                        .unwrap_or_default();
                    if action_button(
                        ui,
                        "Set upstream",
                        ActionButtonKind::Secondary,
                        !current_branch.is_empty() && !state.busy,
                        Some("Current branch is required"),
                    )
                    .clicked()
                    {
                        self.preview_or_confirm(
                            "remote.push_set_upstream",
                            vec![remote.name.clone(), current_branch],
                            "Preview upstream push".to_string(),
                            "Push current branch and set upstream?".to_string(),
                        );
                    }
                    if action_button(
                        ui,
                        "Remove",
                        ActionButtonKind::Danger,
                        !state.busy,
                        Some("Removing a remote requires confirmation"),
                    )
                    .clicked()
                    {
                        self.preview_or_confirm(
                            "remote.remove",
                            vec![remote.name.clone()],
                            "Preview remote remove".to_string(),
                            format!("Remove remote {}?", remote.name),
                        );
                    }
                });
            });
            ui.add_space(design_tokens::SPACING_XS);
        }

        if let Some(upstream) = snapshot.remotes.upstream.as_ref() {
            ui.separator();
            ui.label(format!(
                "Current: {} -> {} (+{} / -{})",
                upstream.current_branch.as_deref().unwrap_or("<detached>"),
                upstream.upstream.as_deref().unwrap_or("<none>"),
                upstream.ahead,
                upstream.behind
            ));
        }

        ui.separator();
        ui.label(RichText::new("Authentication").strong());
        ui.label(format!(
            "SSH agent: {}",
            bool_word(snapshot.remotes.auth.ssh_agent_available)
        ));
        ui.label(format!(
            "Credential helper: {}",
            bool_word(snapshot.remotes.auth.https_helper_configured)
        ));
        if let Some(error) = snapshot.remotes.auth.last_error.as_deref() {
            render_error_state(ui, "Authentication check failed.", error);
        }
        ui.horizontal(|ui| {
            if action_button(
                ui,
                "Auth status",
                ActionButtonKind::Secondary,
                !state.busy,
                None,
            )
            .clicked()
            {
                self.execute_action_direct("auth.status", Vec::new(), false);
            }
            if action_button(
                ui,
                "Manage credentials",
                ActionButtonKind::Secondary,
                !state.busy,
                None,
            )
            .clicked()
            {
                self.show_credentials_dialog = true;
            }
        });
        if !snapshot.remotes.auth.accounts.is_empty() {
            for account in &snapshot.remotes.auth.accounts {
                ui.horizontal_wrapped(|ui| {
                    ui.label(account.host.as_str());
                    ui.label(account.username.as_deref().unwrap_or("<unknown>"));
                    ui.label(format!("{:?}", account.provider));
                    ui.label(if account.token_present {
                        "token stored"
                    } else {
                        "token missing"
                    });
                    let username = account.username.clone().unwrap_or_default();
                    if action_button(
                        ui,
                        "Seed Git",
                        ActionButtonKind::Secondary,
                        account.token_present && !state.busy,
                        Some("Stored token is required"),
                    )
                    .clicked()
                    {
                        self.execute_action_direct(
                            "auth.seed_git",
                            vec![account.host.clone(), username.clone()],
                            false,
                        );
                    }
                    if action_button(ui, "Logout", ActionButtonKind::Danger, !state.busy, None)
                        .clicked()
                    {
                        self.preview_or_confirm(
                            "auth.logout",
                            vec![account.host.clone(), username],
                            "Preview credential removal".to_string(),
                            "Remove this stored credential?".to_string(),
                        );
                    }
                });
            }
        }
        if let Some(message) = snapshot.remotes.last_sync_message.as_deref() {
            ui.label(message);
        }

        if !snapshot.remotes.remote_branches.is_empty() {
            ui.separator();
            ui.label(RichText::new("Remote Branches").strong());
            for branch in snapshot.remotes.remote_branches.iter().take(80) {
                ui.label(format!(
                    "{}/{} {}",
                    branch.remote,
                    branch.name,
                    short_oid(&branch.oid)
                ));
            }
        }
    }

    fn render_workspaces_panel(&mut self, ui: &mut egui::Ui, state: &RuntimeAdapterState) {
        let snapshot = &state.snapshot;
        render_panel_header(
            ui,
            "Workspaces",
            &format!("{} workspaces", snapshot.workspace.workspaces.len()),
        );
        render_panel_notice(ui, PanelId::Workspaces);
        ui.add_space(design_tokens::SPACING_SM);
        ui.horizontal(|ui| {
            ui.add(
                egui::TextEdit::singleline(&mut self.workspace_name_input)
                    .desired_width(220.0)
                    .hint_text("Workspace name"),
            );
            if action_button(
                ui,
                "Create",
                ActionButtonKind::Secondary,
                !self.workspace_name_input.trim().is_empty() && !state.busy,
                Some("Workspace name is required"),
            )
            .clicked()
            {
                self.execute_action_direct(
                    "workspace.create",
                    vec![self.workspace_name_input.trim().to_string()],
                    false,
                );
            }
            if action_button(
                ui,
                "Refresh all",
                ActionButtonKind::Secondary,
                !state.busy,
                None,
            )
            .clicked()
            {
                self.execute_action_direct("workspace.refresh_all", Vec::new(), false);
            }
            if action_button(
                ui,
                "Fetch all",
                ActionButtonKind::Secondary,
                !state.busy,
                None,
            )
            .clicked()
            {
                self.execute_action_direct("workspace.fetch_all", Vec::new(), false);
            }
        });
        ui.horizontal(|ui| {
            ui.add(
                egui::TextEdit::singleline(&mut self.workspace_repo_input)
                    .desired_width(420.0)
                    .hint_text("Repository path"),
            );
            if action_button(
                ui,
                "Add repo",
                ActionButtonKind::Secondary,
                !self.workspace_repo_input.trim().is_empty() && !state.busy,
                Some("Repository path is required"),
            )
            .clicked()
            {
                self.execute_action_direct(
                    "workspace.add_repo",
                    vec![self.workspace_repo_input.trim().to_string()],
                    false,
                );
            }
            if let Some(repo) = snapshot.repo.as_ref()
                && action_button(
                    ui,
                    "Add current",
                    ActionButtonKind::Secondary,
                    !state.busy,
                    None,
                )
                .clicked()
            {
                self.execute_action_direct("workspace.add_repo", vec![repo.root.clone()], false);
            }
        });

        if snapshot.workspace.workspaces.is_empty() {
            render_empty_state(
                ui,
                "No workspaces",
                "Workspace orchestration is experimental and stays out of the core workflow.",
            );
            return;
        }

        ui.add_space(design_tokens::SPACING_MD);
        for workspace in &snapshot.workspace.workspaces {
            let active =
                snapshot.workspace.active_workspace_id.as_deref() == Some(workspace.id.as_str());
            egui::Frame::group(ui.style()).show(ui, |ui| {
                ui.label(
                    RichText::new(format!("{} ({})", workspace.name, workspace.repos.len()))
                        .strong(),
                );
                if active {
                    ui.label("active");
                }
                if action_button(
                    ui,
                    "Switch",
                    ActionButtonKind::Secondary,
                    !active && !state.busy,
                    Some("Workspace is already active"),
                )
                .clicked()
                {
                    self.execute_action_direct(
                        "workspace.switch",
                        vec![workspace.id.clone()],
                        false,
                    );
                }
                for repo in &workspace.repos {
                    ui.horizontal_wrapped(|ui| {
                        ui.label(RichText::new(repo.display_name.as_str()).strong());
                        ui.label(repo.branch_summary.current_branch.as_deref().unwrap_or(""));
                        ui.label(if repo.status_summary.dirty {
                            "dirty"
                        } else {
                            "clean"
                        });
                        ui.label(format!(
                            "+{} / -{}",
                            repo.branch_summary.ahead, repo.branch_summary.behind
                        ));
                        if action_button(ui, "Open", ActionButtonKind::Secondary, !state.busy, None)
                            .clicked()
                        {
                            self.execute_action_direct(
                                "workspace.switch_repo",
                                vec![repo.repo_id.clone()],
                                false,
                            );
                        }
                        if action_button(
                            ui,
                            "Remove",
                            ActionButtonKind::Danger,
                            !state.busy,
                            Some("Removing a repo from a workspace requires confirmation"),
                        )
                        .clicked()
                        {
                            self.preview_or_confirm(
                                "workspace.remove_repo",
                                vec![repo.repo_id.clone()],
                                "Preview workspace removal".to_string(),
                                "Remove repository from this workspace?".to_string(),
                            );
                        }
                    });
                }
            });
            ui.add_space(design_tokens::SPACING_XS);
        }
        if !snapshot.workspace.last_results.is_empty() {
            ui.separator();
            ui.label(RichText::new("Last Results").strong());
            for result in snapshot.workspace.last_results.iter().take(30) {
                ui.label(format!(
                    "{} {} {}",
                    result.repo_id,
                    result.op,
                    if result.success {
                        "ok"
                    } else {
                        result.message.as_str()
                    }
                ));
            }
        }
    }

    fn render_pull_requests_panel(&mut self, ui: &mut egui::Ui, state: &RuntimeAdapterState) {
        let snapshot = &state.snapshot;
        render_panel_header(
            ui,
            "Pull Requests",
            &format!(
                "{} provider items",
                snapshot.pull_requests.pull_requests.len()
            ),
        );
        render_panel_notice(ui, PanelId::PullRequests);
        ui.horizontal(|ui| {
            if action_button(
                ui,
                "Detect provider",
                ActionButtonKind::Secondary,
                !state.busy,
                None,
            )
            .clicked()
            {
                self.execute_action_direct("pr.detect_provider", Vec::new(), false);
            }
            if action_button(ui, "List", ActionButtonKind::Secondary, !state.busy, None).clicked() {
                let args = if self.pr_base_input.trim().is_empty() {
                    Vec::new()
                } else {
                    vec![self.pr_base_input.trim().to_string()]
                };
                self.execute_action_direct("pr.list", args, false);
            }
        });
        ui.horizontal(|ui| {
            ui.add(
                egui::TextEdit::singleline(&mut self.pr_base_input)
                    .desired_width(120.0)
                    .hint_text("base branch"),
            );
            ui.add(
                egui::TextEdit::singleline(&mut self.pr_head_input)
                    .desired_width(180.0)
                    .hint_text("head branch"),
            );
            ui.add(
                egui::TextEdit::singleline(&mut self.pr_title_input)
                    .desired_width(260.0)
                    .hint_text("title"),
            );
            if action_button(
                ui,
                "Create URL",
                ActionButtonKind::Secondary,
                !state.busy,
                None,
            )
            .clicked()
            {
                let mut args = Vec::new();
                if !self.pr_base_input.trim().is_empty() {
                    args.push(self.pr_base_input.trim().to_string());
                }
                if !self.pr_head_input.trim().is_empty() {
                    args.push(self.pr_head_input.trim().to_string());
                } else if !self.pr_title_input.trim().is_empty()
                    && let Some(head) = snapshot.repo.as_ref().and_then(|repo| repo.head.clone())
                {
                    args.push(head);
                }
                if !self.pr_title_input.trim().is_empty() {
                    args.push(self.pr_title_input.trim().to_string());
                }
                self.execute_action_direct("pr.create_url", args, false);
            }
            if action_button(ui, "Open URL", ActionButtonKind::Ghost, !state.busy, None).clicked() {
                self.execute_action_direct("pr.open", Vec::new(), false);
            }
        });
        ui.horizontal(|ui| {
            ui.add(
                egui::TextEdit::singleline(&mut self.pr_number_input)
                    .desired_width(120.0)
                    .hint_text("PR/MR #"),
            );
            if action_button(
                ui,
                "Checkout",
                ActionButtonKind::Primary,
                !self.pr_number_input.trim().is_empty() && !state.busy,
                Some("PR/MR number is required"),
            )
            .clicked()
            {
                self.preview_or_confirm(
                    "pr.checkout",
                    vec![self.pr_number_input.trim().to_string()],
                    "Preview PR checkout".to_string(),
                    "Fetch and checkout this pull request?".to_string(),
                );
            }
        });
        if let Some(provider) = snapshot.pull_requests.detected_provider.as_ref() {
            ui.separator();
            ui.label(format!(
                "Provider: {:?} {}",
                provider.provider, provider.web_url
            ));
        }
        if snapshot.pull_requests.pull_requests.is_empty() {
            render_empty_state(
                ui,
                "Pull Requests are experimental.",
                "Provider integration is available in Advanced mode, but this is not a core workflow yet.",
            );
        } else {
            ui.add_space(design_tokens::SPACING_MD);
        }
        for pr in &snapshot.pull_requests.pull_requests {
            egui::Frame::group(ui.style()).show(ui, |ui| {
                ui.label(format!("{} -> {}", pr.source_branch, pr.target_branch));
                ui.label(format!("{:?}", pr.state));
                ui.label(RichText::new(format!("#{} {}", pr.number, pr.title)).strong());
                if let Some(url) = pr.web_url.as_deref() {
                    ui.label(url);
                }
                for check in &pr.checks {
                    ui.weak(format!("{} {:?}", check.name, check.status));
                }
            });
            ui.add_space(design_tokens::SPACING_XS);
        }
        if let Some(error) = snapshot.pull_requests.last_error.as_deref() {
            render_error_state(ui, "Could not load pull requests.", error);
        }
    }

    fn render_branch_stacks_panel(&mut self, ui: &mut egui::Ui, state: &RuntimeAdapterState) {
        let snapshot = &state.snapshot;
        render_panel_header(
            ui,
            "Branch Stacks",
            &format!("{} stacks", snapshot.branch_stacks.stacks.len()),
        );
        render_panel_notice(ui, PanelId::BranchStacks);
        ui.horizontal(|ui| {
            ui.add(
                egui::TextEdit::singleline(&mut self.stack_base_input)
                    .desired_width(140.0)
                    .hint_text("base ref"),
            );
            if action_button(ui, "Detect", ActionButtonKind::Secondary, !state.busy, None).clicked()
            {
                let args = if self.stack_base_input.trim().is_empty() {
                    Vec::new()
                } else {
                    vec![self.stack_base_input.trim().to_string()]
                };
                self.execute_action_direct("stack.detect", args, false);
            }
        });
        ui.horizontal(|ui| {
            ui.add(
                egui::TextEdit::singleline(&mut self.stack_name_input)
                    .desired_width(180.0)
                    .hint_text("stack name"),
            );
            ui.add(
                egui::TextEdit::singleline(&mut self.stack_branches_input)
                    .desired_width(360.0)
                    .hint_text("branch1 branch2"),
            );
            if action_button(
                ui,
                "Create",
                ActionButtonKind::Secondary,
                !self.stack_name_input.trim().is_empty()
                    && !self.stack_base_input.trim().is_empty()
                    && !self.stack_branches_input.trim().is_empty()
                    && !state.busy,
                Some("Stack name, base ref, and branches are required"),
            )
            .clicked()
            {
                let mut args = vec![
                    self.stack_name_input.trim().to_string(),
                    self.stack_base_input.trim().to_string(),
                ];
                args.extend(
                    self.stack_branches_input
                        .split_whitespace()
                        .map(str::to_string),
                );
                self.execute_action_direct("stack.create", args, false);
            }
            if action_button(
                ui,
                "Restack active",
                ActionButtonKind::Secondary,
                snapshot.branch_stacks.active_stack_id.is_some() && !state.busy,
                Some("No active stack"),
            )
            .clicked()
            {
                let stack_id = snapshot
                    .branch_stacks
                    .active_stack_id
                    .clone()
                    .unwrap_or_default();
                self.preview_or_confirm(
                    "stack.restack",
                    vec![stack_id],
                    "Preview stack restack".to_string(),
                    "Restack active branch stack?".to_string(),
                );
            }
        });
        if snapshot.branch_stacks.stacks.is_empty() {
            render_empty_state(
                ui,
                "No branch stacks",
                "Stacked branch workflows are experimental and stay behind Advanced mode.",
            );
        }
        for stack in &snapshot.branch_stacks.stacks {
            egui::Frame::group(ui.style()).show(ui, |ui| {
                ui.label(
                    RichText::new(format!("{} ({})", stack.name, stack.entries.len())).strong(),
                );
                if snapshot.branch_stacks.active_stack_id.as_deref() == Some(stack.id.as_str()) {
                    ui.label("active");
                }
                if action_button(
                    ui,
                    "Restack",
                    ActionButtonKind::Secondary,
                    !state.busy,
                    None,
                )
                .clicked()
                {
                    self.preview_or_confirm(
                        "stack.restack",
                        vec![stack.id.clone()],
                        "Preview stack restack".to_string(),
                        format!("Restack {}?", stack.name),
                    );
                }
                for entry in &stack.entries {
                    ui.horizontal_wrapped(|ui| {
                        ui.label(RichText::new(entry.branch.as_str()).monospace());
                        ui.label(format!("base: {}", entry.base_branch));
                        ui.label(format!("+{} / -{}", entry.ahead, entry.behind));
                        ui.label(format!("{:?}", entry.status));
                    });
                }
            });
            ui.add_space(design_tokens::SPACING_XS);
        }
        ui.separator();
        ui.label(RichText::new("Virtual Branches").strong());
        ui.horizontal(|ui| {
            ui.add(
                egui::TextEdit::singleline(&mut self.virtual_branch_name_input)
                    .desired_width(180.0)
                    .hint_text("context name"),
            );
            ui.add(
                egui::TextEdit::singleline(&mut self.virtual_branch_paths_input)
                    .desired_width(360.0)
                    .hint_text("optional paths"),
            );
            let paths = self
                .virtual_branch_paths_input
                .split_whitespace()
                .map(str::to_string)
                .collect::<Vec<_>>();
            if action_button(ui, "Detect", ActionButtonKind::Secondary, !state.busy, None).clicked()
            {
                let mut args = Vec::new();
                if !self.virtual_branch_name_input.trim().is_empty() {
                    args.push(self.virtual_branch_name_input.trim().to_string());
                }
                args.extend(paths.clone());
                self.execute_action_direct("virtual.detect", args, false);
            }
            if action_button(
                ui,
                "Create",
                ActionButtonKind::Secondary,
                !state.busy
                    && !self.virtual_branch_name_input.trim().is_empty()
                    && !paths.is_empty(),
                Some("Context name and paths are required"),
            )
            .clicked()
            {
                let mut args = vec![self.virtual_branch_name_input.trim().to_string()];
                args.extend(paths);
                self.execute_action_direct("virtual.create", args, false);
            }
        });
        if !snapshot.virtual_branches.branches.is_empty() {
            egui::Grid::new("virtual.branches")
                .striped(true)
                .show(ui, |ui| {
                    ui.label(RichText::new("Context").strong());
                    ui.label(RichText::new("Base").strong());
                    ui.label(RichText::new("Files").strong());
                    ui.label(RichText::new("Status").strong());
                    ui.label(RichText::new("Actions").strong());
                    ui.end_row();
                    for branch in &snapshot.virtual_branches.branches {
                        let active = snapshot.virtual_branches.active_branch_id.as_deref()
                            == Some(branch.id.as_str());
                        ui.label(if active {
                            format!("{} active", branch.name)
                        } else {
                            branch.name.clone()
                        });
                        ui.label(branch.base_branch.as_deref().unwrap_or("<detached>"));
                        ui.label(branch.changes.len().to_string());
                        ui.label(format!("{:?}", branch.status));
                        ui.horizontal(|ui| {
                            if ui
                                .add_enabled(!state.busy, egui::Button::new("Switch"))
                                .clicked()
                            {
                                self.execute_action_direct(
                                    "virtual.switch",
                                    vec![branch.id.clone()],
                                    false,
                                );
                            }
                            if ui
                                .add_enabled(!state.busy, egui::Button::new("Export"))
                                .clicked()
                            {
                                self.execute_action_direct(
                                    "virtual.export_patch",
                                    vec![branch.id.clone()],
                                    false,
                                );
                            }
                        });
                        ui.end_row();
                    }
                });
        }
        if let Some(export) = snapshot.virtual_branches.last_export.as_deref() {
            ui.label(format!("Last virtual export: {export}"));
        }
        if let Some(error) = snapshot.virtual_branches.last_error.as_deref() {
            ui.colored_label(Color32::from_rgb(248, 113, 113), error);
        }
        if let Some(error) = snapshot.branch_stacks.last_error.as_deref() {
            ui.colored_label(Color32::from_rgb(248, 113, 113), error);
        }
    }

    fn render_stash_panel(&mut self, ui: &mut egui::Ui, state: &RuntimeAdapterState) {
        let snapshot = &state.snapshot;
        render_panel_header(ui, "Stash", "Advanced recovery tool for local work.");
        render_panel_notice(ui, PanelId::Stash);
        ui.add_space(design_tokens::SPACING_SM);
        ui.label(RichText::new("Create stash").strong());
        ui.horizontal_wrapped(|ui| {
            ui.add(
                egui::TextEdit::singleline(&mut self.stash_message_input)
                    .desired_width(320.0)
                    .hint_text("Stash message"),
            );
            if action_button(
                ui,
                "Create",
                ActionButtonKind::Primary,
                status_is_dirty(snapshot) && !state.busy,
                Some("No local changes to stash"),
            )
            .clicked()
            {
                let message = self.stash_message_input.trim().to_string();
                let args = if message.is_empty() {
                    Vec::new()
                } else {
                    vec![message]
                };
                self.preview_or_confirm(
                    "stash.create",
                    args,
                    "Preview stash create".to_string(),
                    "Create a stash from current local changes?".to_string(),
                );
            }
            if action_button(ui, "List", ActionButtonKind::Secondary, !state.busy, None).clicked() {
                self.execute_action_direct("stash.list", Vec::new(), false);
            }
        });

        ui.add_space(design_tokens::SPACING_MD);
        ui.label(RichText::new("Selected stash").strong());
        ui.horizontal_wrapped(|ui| {
            ui.add(
                egui::TextEdit::singleline(&mut self.stash_selector_input)
                    .desired_width(180.0)
                    .hint_text("stash@{0}"),
            );
            let selector = self.stash_selector_input.trim().to_string();
            let has_selector = !selector.is_empty();
            if action_button(
                ui,
                "Apply",
                ActionButtonKind::Secondary,
                has_selector && !state.busy,
                Some("Enter a stash selector such as stash@{0}"),
            )
            .clicked()
            {
                let fallback = if status_is_dirty(snapshot) {
                    "Working tree has local changes. Applying a stash may conflict.".to_string()
                } else {
                    format!("Apply {selector} to the working tree?")
                };
                self.preview_or_confirm(
                    "stash.apply",
                    vec![selector.clone()],
                    "Preview stash apply".to_string(),
                    fallback,
                );
            }
            if action_button(
                ui,
                "Pop",
                ActionButtonKind::Danger,
                has_selector && !state.busy,
                Some("Pop applies and removes the stash after a successful apply"),
            )
            .clicked()
            {
                self.preview_or_confirm(
                    "stash.pop",
                    vec![selector.clone()],
                    "Preview stash pop".to_string(),
                    format!("Apply {selector} and remove it from the stash list?"),
                );
            }
            if action_button(
                ui,
                "Drop",
                ActionButtonKind::Danger,
                has_selector && !state.busy,
                Some("Drop removes a stash entry and requires confirmation"),
            )
            .clicked()
            {
                self.preview_or_confirm(
                    "stash.drop",
                    vec![selector.clone()],
                    "Preview stash drop".to_string(),
                    format!("Remove {selector} from the stash list?"),
                );
            }
        });

        ui.add_space(design_tokens::SPACING_MD);
        ui.label(RichText::new("Stash entries").strong());
        if let Some(content) = snapshot.diff.content.as_deref().filter(|_| {
            matches!(
                snapshot.diff.source.as_ref(),
                Some(DiffSource::Commit { oid }) if oid == "stash:list"
            )
        }) {
            if content.trim() == "stash: <empty>" {
                render_empty_state(
                    ui,
                    "No stashes",
                    "Create a stash to preserve local changes for later.",
                );
            } else {
                for line in content.lines() {
                    let (reference, message) = line.split_once(' ').unwrap_or((line, ""));
                    let selected = self.stash_selector_input.trim() == reference;
                    egui::Frame::group(ui.style()).show(ui, |ui| {
                        if ui
                            .selectable_label(selected, RichText::new(reference).monospace())
                            .clicked()
                        {
                            self.stash_selector_input = reference.to_string();
                        }
                        if !message.is_empty() {
                            ui.label(message);
                        }
                    });
                    ui.add_space(design_tokens::SPACING_XS);
                }
            }
        } else {
            render_empty_state(
                ui,
                "Stash list not loaded",
                "Click List to load current stash entries.",
            );
        }
    }

    fn render_simple_action_panel(
        &mut self,
        ui: &mut egui::Ui,
        state: &RuntimeAdapterState,
        title: &str,
        actions: &[(&str, &str)],
    ) {
        render_panel_header(ui, title, "This panel is experimental.");
        if let Some(panel) = panel_for_title(title) {
            render_panel_notice(ui, panel);
        }
        ui.horizontal(|ui| {
            for (label, action_id) in actions {
                if action_button(ui, label, ActionButtonKind::Secondary, !state.busy, None)
                    .clicked()
                {
                    self.execute_action_direct(action_id, Vec::new(), false);
                }
            }
        });
        if let Some(content) = state.snapshot.diff.content.as_deref() {
            render_diff_text(ui, content);
        } else {
            render_empty_state(
                ui,
                "This panel is experimental.",
                "Some actions may be unavailable until the workflow is stabilized.",
            );
        }
    }

    fn render_conflicts_panel(&mut self, ui: &mut egui::Ui, state: &RuntimeAdapterState) {
        let snapshot = &state.snapshot;
        render_panel_header(ui, "Conflicts", "Advanced conflict recovery tools.");
        render_panel_notice(ui, PanelId::Conflicts);
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
                render_empty_state(
                    ui,
                    "No active conflict session.",
                    "Conflict controls are available when merge, rebase, or cherry-pick markers exist.",
                );
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
            if action_button(
                ui,
                "List conflicts",
                ActionButtonKind::Secondary,
                !state.busy,
                None,
            )
            .clicked()
            {
                self.execute_action_direct("conflict.list", Vec::new(), false);
            }
            if let Some(file) = self.selected_file(snapshot) {
                if action_button(ui, "Focus", ActionButtonKind::Secondary, !state.busy, None)
                    .clicked()
                {
                    self.execute_action_direct("conflict.focus", vec![file.to_string()], false);
                }
                if action_button(ui, "Use ours", ActionButtonKind::Secondary, !state.busy, None)
                    .clicked()
                {
                    self.execute_action_direct(
                        "conflict.resolve.ours",
                        vec![file.to_string()],
                        false,
                    );
                }
                if action_button(ui, "Use theirs", ActionButtonKind::Secondary, !state.busy, None)
                    .clicked()
                {
                    self.execute_action_direct(
                        "conflict.resolve.theirs",
                        vec![file.to_string()],
                        false,
                    );
                }
                if action_button(
                    ui,
                    "Mark resolved",
                    ActionButtonKind::Secondary,
                    !state.busy,
                    None,
                )
                .clicked()
                {
                    self.execute_action_direct(
                        "conflict.mark_resolved",
                        vec![file.to_string()],
                        false,
                    );
                }
            }
            if action_button(
                ui,
                "Continue",
                ActionButtonKind::Primary,
                !state.busy,
                Some("Continue the active Git operation"),
            )
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
            if action_button(
                ui,
                "Abort",
                ActionButtonKind::Danger,
                !state.busy,
                Some("Abort requires confirmation"),
            )
            .clicked()
            {
                self.preview_or_confirm(
                    "conflict.abort",
                    Vec::new(),
                    "Preview abort".to_string(),
                    "Abort the active conflict session?".to_string(),
                );
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
        render_panel_header(
            ui,
            "Diagnostics",
            "Developer-only runtime and repository diagnostics.",
        );
        render_panel_notice(ui, PanelId::Diagnostics);
        ui.add_space(design_tokens::SPACING_SM);
        ui.label(RichText::new("State Snapshot Summary").strong());
        ui.label(format!("version: {}", state.snapshot.version));
        ui.label(format!(
            "status: staged={} unstaged={} untracked={}",
            state.snapshot.status.staged.len(),
            state.snapshot.status.unstaged.len(),
            state.snapshot.status.untracked.len()
        ));
        ui.label(format!(
            "history_commits: {}",
            state.snapshot.history.commits.len()
        ));
        ui.label(format!("diff_hunks: {}", state.snapshot.diff.hunks.len()));
        ui.label(format!(
            "journal_entries: {}",
            state.snapshot.journal.entries.len()
        ));
        ui.separator();
        ui.label(RichText::new("RepoContext").strong());
        if let Some(repo) = state.snapshot.repo.as_ref() {
            ui.label(format!("repo_id: {}", repo.root));
            ui.label(format!("repo_path: {}", repo.root));
            ui.label(format!(
                "current_branch: {}",
                repo.head.as_deref().unwrap_or("detached HEAD")
            ));
        } else {
            ui.weak("repo_id: <none>");
        }
        ui.label(RichText::new("SelectionState").strong());
        ui.label(format!(
            "files: {}",
            if state.snapshot.selection.selected_paths.is_empty() {
                "<none>".to_string()
            } else {
                state.snapshot.selection.selected_paths.join(", ")
            }
        ));
        ui.label(format!(
            "commit: {}",
            state
                .snapshot
                .selection
                .selected_commit_oid
                .as_deref()
                .unwrap_or("<none>")
        ));
        ui.label(format!(
            "branch: {}",
            state
                .snapshot
                .selection
                .selected_branch
                .as_deref()
                .unwrap_or("<none>")
        ));
        ui.label(format!(
            "active_panel: {}",
            self.ui_state.active_panel.label()
        ));
        ui.label(format!(
            "active_diff_source: {}",
            state
                .snapshot
                .diff
                .source
                .as_ref()
                .map(format_diff_source)
                .unwrap_or_else(|| "<none>".to_string())
        ));
        ui.separator();
        ui.label(RichText::new("Feature Flags").strong());
        ui.label(format!(
            "advanced_mode: {}",
            self.ui_state.layout.advanced_mode
        ));
        ui.label(format!(
            "developer_mode: {}",
            self.ui_state.layout.developer_mode
        ));
        ui.label(format!("dark_mode: {}", self.ui_state.layout.dark_mode));
        ui.separator();
        ui.label(RichText::new("Job Queue").strong());
        ui.label(format!("busy: {}", state.busy));
        ui.label(format!(
            "current_operation: {}",
            state.current_operation.as_deref().unwrap_or("<none>")
        ));
        ui.separator();
        ui.label(RichText::new("Capabilities").strong());
        if let Some(capabilities) = state.snapshot.repo_capabilities.as_ref() {
            ui.label(format!(
                "linked_worktree: {}",
                capabilities.is_linked_worktree
            ));
            ui.label(format!("submodules: {}", capabilities.has_submodules));
            ui.label(format!("lfs_detected: {}", capabilities.lfs_detected));
            ui.label(format!("lfs_available: {}", capabilities.lfs_available));
        } else {
            ui.weak("<none>");
        }
        ui.separator();
        ui.label(format!("Action catalog: {}", state.action_catalog.len()));
        ui.label(format!("Plugins: {}", state.snapshot.plugins.len()));
        for plugin in &state.snapshot.plugins {
            ui.label(format!("{}: {:?}", plugin.plugin_id, plugin.health));
        }
        ui.separator();
        ui.horizontal(|ui| {
            ui.add(
                egui::TextEdit::singleline(&mut self.plugin_registry_input)
                    .desired_width(320.0)
                    .hint_text("registry path or URL"),
            );
            if action_button(
                ui,
                "Marketplace",
                ActionButtonKind::Secondary,
                !state.busy,
                None,
            )
            .clicked()
            {
                let args = if self.plugin_registry_input.trim().is_empty() {
                    Vec::new()
                } else {
                    vec![self.plugin_registry_input.trim().to_string()]
                };
                self.execute_action_direct("plugin.marketplace", args, false);
            }
            if action_button(
                ui,
                "Discover",
                ActionButtonKind::Secondary,
                !state.busy,
                None,
            )
            .clicked()
            {
                let args = if self.plugin_registry_input.trim().is_empty() {
                    Vec::new()
                } else {
                    vec![self.plugin_registry_input.trim().to_string()]
                };
                self.execute_action_direct("plugin.discover", args, false);
            }
        });
        ui.horizontal(|ui| {
            ui.add(
                egui::TextEdit::singleline(&mut self.plugin_id_input)
                    .desired_width(220.0)
                    .hint_text("plugin id"),
            );
            if let Some(selected) = state.snapshot.selection.selected_plugin_id.as_deref()
                && self.plugin_id_input.trim().is_empty()
            {
                self.plugin_id_input = selected.to_string();
            }
            let registry = self.plugin_registry_input.trim().to_string();
            if action_button(
                ui,
                "Install",
                ActionButtonKind::Secondary,
                !self.plugin_id_input.trim().is_empty() && !state.busy,
                Some("Plugin id is required"),
            )
            .clicked()
            {
                let mut args = vec![self.plugin_id_input.trim().to_string()];
                if !registry.is_empty() {
                    args.push(registry.clone());
                }
                self.execute_action_direct("plugin.install_registry", args, false);
            }
            if action_button(
                ui,
                "Update",
                ActionButtonKind::Secondary,
                !self.plugin_id_input.trim().is_empty() && !state.busy,
                Some("Plugin id is required"),
            )
            .clicked()
            {
                let mut args = vec![self.plugin_id_input.trim().to_string()];
                if !registry.is_empty() {
                    args.push(registry);
                }
                self.preview_or_confirm(
                    "plugin.update",
                    args,
                    "Preview plugin update".to_string(),
                    "Update this plugin from the marketplace registry?".to_string(),
                );
            }
        });
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
        if !state.snapshot.plugin_security.is_empty() {
            ui.separator();
            ui.label(RichText::new("Plugin Security").strong());
            egui::Grid::new("plugin.security")
                .striped(true)
                .show(ui, |ui| {
                    ui.label(RichText::new("Plugin").strong());
                    ui.label(RichText::new("Trust").strong());
                    ui.label(RichText::new("Signature").strong());
                    ui.label(RichText::new("Sandbox").strong());
                    ui.label(RichText::new("Update").strong());
                    ui.label(RichText::new("Warnings").strong());
                    ui.end_row();
                    for record in &state.snapshot.plugin_security {
                        let warnings = if record.warnings.is_empty() {
                            String::new()
                        } else {
                            record.warnings.join("; ")
                        };
                        ui.label(record.plugin_id.as_str());
                        ui.label(format!("{:?}", record.trust_level));
                        ui.label(format!("{:?}", record.signature_status));
                        ui.label(record.sandbox_mode.as_str());
                        ui.label(if record.update_available { "yes" } else { "no" });
                        ui.label(warnings);
                        ui.end_row();
                    }
                });
        }
        ui.separator();
        ui.label(RichText::new("Event Log").strong());
        for entry in state.snapshot.journal.entries.iter().rev().take(8) {
            ui.label(format!("#{} {} {:?}", entry.id, entry.op, entry.status));
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
                ui.label(RichText::new("Disabled reason").strong());
                ui.end_row();
                for item in &state.action_catalog {
                    ui.label(if item.enabled { "on" } else { "off" });
                    ui.label(item.action_id.as_str());
                    ui.label(item.owner.as_str());
                    ui.label(format_danger(&item.danger));
                    ui.label(item.disabled_reason.as_deref().unwrap_or(""));
                    ui.end_row();
                }
            });
    }

    fn render_journal_panel(&mut self, ui: &mut egui::Ui, state: &RuntimeAdapterState) {
        render_panel_header(ui, "Journal", "Operation recovery and safety preview.");
        render_panel_notice(ui, PanelId::Journal);
        ui.separator();
        ui.horizontal(|ui| {
            ui.add(
                egui::TextEdit::singleline(&mut self.ui_state.journal_view_state.filter)
                    .desired_width(220.0)
                    .hint_text("Filter"),
            );
            ui.checkbox(
                &mut self.ui_state.journal_view_state.recovery_only,
                "Recovery available",
            );
            if ui
                .add_enabled(!state.busy, egui::Button::new("Export"))
                .clicked()
            {
                self.execute_action_direct("journal.export", Vec::new(), false);
            }
            if ui
                .add_enabled(!state.busy, egui::Button::new("Keep latest 50"))
                .clicked()
            {
                self.execute_action_direct(
                    "journal.clear_old_entries",
                    vec!["50".to_string()],
                    false,
                );
            }
        });
        ui.separator();

        if state.snapshot.journal.entries.is_empty() {
            render_empty_state(
                ui,
                "No operations recorded yet.",
                "When repository-changing actions are performed, they will appear here.",
            );
        } else {
            let filter = self.ui_state.journal_view_state.filter.to_lowercase();
            let entries = state
                .snapshot
                .journal
                .entries
                .iter()
                .rev()
                .filter(|entry| {
                    (filter.is_empty()
                        || entry.op.to_lowercase().contains(&filter)
                        || entry
                            .error
                            .as_deref()
                            .unwrap_or_default()
                            .to_lowercase()
                            .contains(&filter))
                        && (!self.ui_state.journal_view_state.recovery_only
                            || !entry.backup_refs.is_empty())
                })
                .cloned()
                .collect::<Vec<_>>();

            egui::Grid::new("journal.entries")
                .striped(true)
                .min_col_width(80.0)
                .show(ui, |ui| {
                    ui.label(RichText::new("Time").strong());
                    ui.label(RichText::new("Operation").strong());
                    ui.label(RichText::new("Status").strong());
                    ui.label(RichText::new("Risk").strong());
                    ui.label(RichText::new("Recovery").strong());
                    ui.label(RichText::new("Actions").strong());
                    ui.end_row();

                    for entry in &entries {
                        let selected =
                            self.ui_state.journal_view_state.selected_entry_id == Some(entry.id);
                        if ui
                            .selectable_label(selected, entry.started_at_ms.to_string())
                            .clicked()
                        {
                            self.ui_state.journal_view_state.selected_entry_id = Some(entry.id);
                        }
                        ui.label(entry.op.as_str());
                        ui.colored_label(
                            journal_status_color(entry),
                            format!("{:?}", entry.status),
                        );
                        ui.label(entry.risk.as_ref().map(format_danger).unwrap_or("unknown"));
                        ui.label(if entry.backup_refs.is_empty() {
                            ""
                        } else {
                            "available"
                        });
                        ui.horizontal(|ui| {
                            if ui.button("Details").clicked() {
                                self.ui_state.journal_view_state.selected_entry_id = Some(entry.id);
                                self.execute_action_direct(
                                    "journal.open_entry",
                                    vec![entry.id.to_string()],
                                    false,
                                );
                            }
                            if ui
                                .add_enabled(
                                    !entry.backup_refs.is_empty() && !state.busy,
                                    egui::Button::new("Restore ref"),
                                )
                                .clicked()
                            {
                                self.preview_or_confirm(
                                    "journal.restore_ref",
                                    vec![entry.id.to_string()],
                                    "Preview ref restore".to_string(),
                                    "Restore the saved ref from this journal entry?".to_string(),
                                );
                            }
                            if ui
                                .add_enabled(
                                    !entry.backup_refs.is_empty() && !state.busy,
                                    egui::Button::new("Recovery branch"),
                                )
                                .clicked()
                            {
                                let branch = recovery_branch_name(
                                    &self.ui_state.journal_view_state.recovery_branch_name,
                                    entry.id,
                                );
                                self.preview_or_confirm(
                                    "journal.recover_operation",
                                    vec![entry.id.to_string(), branch],
                                    "Preview recovery branch".to_string(),
                                    "Create a recovery branch from the backup ref?".to_string(),
                                );
                            }
                        });
                        ui.end_row();
                    }
                });
        }

        if let Some(entry) = self
            .ui_state
            .journal_view_state
            .selected_entry_id
            .and_then(|id| {
                state
                    .snapshot
                    .journal
                    .entries
                    .iter()
                    .find(|entry| entry.id == id)
            })
        {
            ui.separator();
            render_journal_entry(ui, entry);
        }

        ui.separator();
        ui.label(RichText::new("Recovery").strong());
        ui.horizontal(|ui| {
            ui.add(
                egui::TextEdit::singleline(&mut self.ui_state.journal_view_state.reflog_reference)
                    .desired_width(160.0)
                    .hint_text("HEAD"),
            );
            ui.add(
                egui::TextEdit::singleline(
                    &mut self.ui_state.journal_view_state.recovery_branch_name,
                )
                .desired_width(220.0)
                .hint_text("branchforge/recovery"),
            );
            if ui
                .add_enabled(!state.busy, egui::Button::new("Load reflog"))
                .clicked()
            {
                let reference = self.ui_state.journal_view_state.reflog_reference.clone();
                match self.runtime.load_reflog(&reference, 40) {
                    Ok(_) => {}
                    Err(error) => self.ui_error = Some(error.to_string()),
                }
            }
        });
        for (idx, entry) in state.reflog_entries.iter().enumerate() {
            ui.horizontal(|ui| {
                ui.label(short_oid(&entry.oid));
                ui.label(entry.selector.as_str());
                ui.label(entry.time.as_str());
                ui.label(entry.message.as_str());
                if ui
                    .add_enabled(!state.busy, egui::Button::new("Branch here"))
                    .clicked()
                {
                    let branch = format!(
                        "{}-{}",
                        recovery_branch_name(
                            &self.ui_state.journal_view_state.recovery_branch_name,
                            0
                        ),
                        idx + 1
                    );
                    self.preview_or_confirm(
                        "recovery.create_branch_from_reflog",
                        vec![branch, entry.oid.clone()],
                        "Preview reflog branch".to_string(),
                        "Create a branch at this reflog entry?".to_string(),
                    );
                }
            });
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
                            ui.vertical(|ui| {
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
                                if let Some(explain) = item.explain.as_ref() {
                                    ui.weak(explain.plain_summary.as_str());
                                    if let Some(command) = explain.git_commands.first() {
                                        ui.label(RichText::new(command).monospace());
                                    }
                                }
                            });
                            ui.separator();
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

    fn render_preview(&mut self, ctx: &egui::Context) {
        let Some(mut dialog) = self.ui_state.pending_preview.clone() else {
            return;
        };

        let mut keep_open = true;
        egui::Window::new(dialog.title.as_str())
            .open(&mut keep_open)
            .collapsible(false)
            .resizable(true)
            .default_width(680.0)
            .show(ctx, |ui| {
                render_operation_preview(ui, &dialog.preview);
                ui.separator();
                ui.checkbox(&mut dialog.understood, "I understand");
                ui.horizontal(|ui| {
                    if ui.button("Cancel").clicked() {
                        self.ui_state.pending_preview = None;
                    }
                    if ui
                        .add_enabled(dialog.understood, egui::Button::new("Confirm"))
                        .clicked()
                    {
                        let result =
                            self.runtime
                                .execute_action(&dialog.action_id, &dialog.args, true);
                        self.record_submit(result);
                        self.ui_state.pending_preview = None;
                    }
                });
            });

        if !keep_open {
            self.ui_state.pending_preview = None;
        } else if self.ui_state.pending_preview.is_some() {
            self.ui_state.pending_preview = Some(dialog);
        }
    }

    fn render_credentials_dialog(&mut self, ctx: &egui::Context, state: &RuntimeAdapterState) {
        if !self.show_credentials_dialog {
            return;
        }

        let mut open = self.show_credentials_dialog;
        egui::Window::new("Manage credentials")
            .open(&mut open)
            .collapsible(false)
            .resizable(false)
            .default_width(460.0)
            .show(ctx, |ui| {
                ui.label("Authentication");
                ui.add(
                    egui::TextEdit::singleline(&mut self.auth_host_input)
                        .desired_width(f32::INFINITY)
                        .hint_text("host"),
                );
                ui.add(
                    egui::TextEdit::singleline(&mut self.auth_username_input)
                        .desired_width(f32::INFINITY)
                        .hint_text("username"),
                );
                ui.add(
                    egui::TextEdit::singleline(&mut self.auth_token_input)
                        .password(true)
                        .desired_width(f32::INFINITY)
                        .hint_text("token"),
                );
                ui.horizontal(|ui| {
                    if action_button(ui, "Cancel", ActionButtonKind::Ghost, true, None).clicked() {
                        self.auth_token_input.clear();
                        self.show_credentials_dialog = false;
                    }
                    if action_button(
                        ui,
                        "Store token",
                        ActionButtonKind::Primary,
                        !self.auth_host_input.trim().is_empty()
                            && !self.auth_username_input.trim().is_empty()
                            && !self.auth_token_input.is_empty()
                            && !state.busy,
                        Some("Host, username, and token are required"),
                    )
                    .clicked()
                    {
                        self.execute_action_direct(
                            "auth.login",
                            vec![
                                self.auth_host_input.trim().to_string(),
                                self.auth_username_input.trim().to_string(),
                                self.auth_token_input.clone(),
                            ],
                            false,
                        );
                        self.auth_token_input.clear();
                        self.show_credentials_dialog = false;
                    }
                });
            });

        if !open {
            self.auth_token_input.clear();
        }
        self.show_credentials_dialog = open && self.show_credentials_dialog;
    }
}

impl eframe::App for BranchForgeDesktopApp {
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        let ctx = ui.ctx().clone();
        self.handle_shortcuts(&ctx);
        let state = self.runtime.state();
        self.sync_local_ui_from_runtime(&state);
        self.render_top_bar(ui, &state);
        self.render_sidebar(ui);
        self.render_inspector(ui, &state);
        self.render_status_bar(ui, &state);
        self.render_main_panel(ui, &state);
        self.render_command_palette(&ctx, &state);
        self.render_credentials_dialog(&ctx, &state);
        self.render_confirmation(&ctx);
        self.render_preview(&ctx);

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

const REPOSITORY_PANELS: &[PanelId] = &[
    PanelId::Status,
    PanelId::History,
    PanelId::Diff,
    PanelId::Branches,
    PanelId::Tags,
];
const COLLABORATION_PANELS: &[PanelId] = &[PanelId::Remotes, PanelId::PullRequests];
const ADVANCED_PANELS: &[PanelId] = &[
    PanelId::Compare,
    PanelId::Stash,
    PanelId::Worktrees,
    PanelId::Submodules,
    PanelId::Conflicts,
    PanelId::Workspaces,
    PanelId::BranchStacks,
];
const DEVELOPER_PANELS: &[PanelId] = &[PanelId::Diagnostics, PanelId::Journal];
const SIDEBAR_SECTIONS: &[(&str, &[PanelId])] = &[
    ("Repository", REPOSITORY_PANELS),
    ("Collaboration", COLLABORATION_PANELS),
    ("Advanced", ADVANCED_PANELS),
    ("Developer", DEVELOPER_PANELS),
];

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum StatusFileGroup {
    Staged,
    Unstaged,
    Untracked,
}

fn sidebar_sections() -> &'static [(&'static str, &'static [PanelId])] {
    SIDEBAR_SECTIONS
}

fn sidebar_badge(panel: PanelId) -> Option<&'static str> {
    match panel.panel_status() {
        PanelStatus::Core => None,
        PanelStatus::Preview => Some("Preview"),
        PanelStatus::Advanced => Some("Experimental"),
        PanelStatus::Developer => Some("Dev"),
        PanelStatus::DeveloperPreview => Some("Preview"),
    }
}

fn action_button(
    ui: &mut egui::Ui,
    label: &str,
    kind: ActionButtonKind,
    enabled: bool,
    tooltip: Option<&str>,
) -> egui::Response {
    let dark = ui.visuals().dark_mode;
    let text_color = match kind {
        ActionButtonKind::Primary => Color32::WHITE,
        ActionButtonKind::Danger => design_tokens::danger(dark),
        ActionButtonKind::Secondary | ActionButtonKind::Ghost => design_tokens::text(dark),
    };
    let mut button = egui::Button::new(
        RichText::new(label)
            .size(design_tokens::FONT_SIZE_SM)
            .color(if enabled {
                text_color
            } else {
                design_tokens::disabled(dark)
            }),
    )
    .min_size(egui::vec2(0.0, design_tokens::BUTTON_HEIGHT_MD));

    button = match kind {
        ActionButtonKind::Primary => button.fill(design_tokens::accent(dark)),
        ActionButtonKind::Secondary => button.fill(design_tokens::surface_alt(dark)),
        ActionButtonKind::Danger => button
            .fill(design_tokens::danger_soft(dark))
            .stroke(egui::Stroke::new(1.0, design_tokens::danger(dark))),
        ActionButtonKind::Ghost => button.fill(Color32::TRANSPARENT),
    };

    let response = ui.add_enabled(enabled, button);
    match tooltip {
        Some(text) if enabled => response.on_hover_text(text),
        Some(text) => response.on_disabled_hover_text(text),
        None => response,
    }
}

fn render_panel_header(ui: &mut egui::Ui, title: &str, summary: &str) {
    ui.horizontal_wrapped(|ui| {
        ui.heading(title);
        if !summary.is_empty() {
            ui.label(
                RichText::new(summary)
                    .size(design_tokens::FONT_SIZE_SM)
                    .color(design_tokens::text_muted(ui.visuals().dark_mode)),
            );
        }
    });
    ui.separator();
}

fn render_empty_state(ui: &mut egui::Ui, title: &str, description: &str) {
    egui::Frame::group(ui.style())
        .fill(design_tokens::surface_alt(ui.visuals().dark_mode))
        .show(ui, |ui| {
            ui.label(RichText::new(title).strong());
            ui.label(
                RichText::new(description).color(design_tokens::text_muted(ui.visuals().dark_mode)),
            );
        });
}

fn render_loading_state(ui: &mut egui::Ui, message: &str) {
    ui.horizontal(|ui| {
        ui.spinner();
        ui.label(message);
    });
}

fn render_error_state(ui: &mut egui::Ui, title: &str, detail: &str) {
    egui::Frame::group(ui.style())
        .fill(design_tokens::danger_soft(ui.visuals().dark_mode))
        .show(ui, |ui| {
            ui.colored_label(design_tokens::danger(ui.visuals().dark_mode), title);
            ui.label(detail);
        });
}

fn repo_display_name(path: &str) -> String {
    Path::new(path)
        .file_name()
        .and_then(|name| name.to_str())
        .filter(|name| !name.is_empty())
        .unwrap_or(path)
        .to_string()
}

fn reveal_repo_path(repo_root: &str, file: &str) -> Result<(), String> {
    let full_path = PathBuf::from(repo_root).join(file);
    #[cfg(target_os = "macos")]
    let status = Command::new("open")
        .arg("-R")
        .arg(&full_path)
        .status()
        .map_err(|error| format!("Could not reveal file: {error}"))?;

    #[cfg(target_os = "windows")]
    let status = Command::new("explorer")
        .arg(format!("/select,{}", full_path.display()))
        .status()
        .map_err(|error| format!("Could not reveal file: {error}"))?;

    #[cfg(all(not(target_os = "macos"), not(target_os = "windows")))]
    let status = Command::new("xdg-open")
        .arg(full_path.parent().unwrap_or_else(|| Path::new(repo_root)))
        .status()
        .map_err(|error| format!("Could not reveal file: {error}"))?;

    if status.success() {
        Ok(())
    } else {
        Err(format!(
            "Could not reveal file: file manager exited with {status}"
        ))
    }
}

fn truncate_middle(value: &str, max_chars: usize) -> String {
    let total = value.chars().count();
    if total <= max_chars || max_chars <= 6 {
        return value.to_string();
    }
    let side = (max_chars - 3) / 2;
    let start = value.chars().take(side).collect::<String>();
    let end_len = max_chars - 3 - side;
    let end = value
        .chars()
        .rev()
        .take(end_len)
        .collect::<Vec<_>>()
        .into_iter()
        .rev()
        .collect::<String>();
    format!("{start}...{end}")
}

fn branch_badge_label(snapshot: &StoreSnapshot) -> Option<String> {
    let repo = snapshot.repo.as_ref()?;
    match repo.head.as_deref() {
        Some(head) => Some(head.to_string()),
        None => {
            let oid = snapshot
                .selection
                .selected_commit_oid
                .as_deref()
                .or_else(|| {
                    snapshot
                        .history
                        .commits
                        .first()
                        .map(|commit| commit.oid.as_str())
                });
            Some(match oid {
                Some(oid) => format!("detached: {}", short_oid(oid)),
                None => "detached HEAD".to_string(),
            })
        }
    }
}

fn dirty_summary(snapshot: &StoreSnapshot) -> String {
    if snapshot
        .repo
        .as_ref()
        .and_then(|repo| repo.conflict_state.as_ref())
        .is_some()
    {
        return "conflicts".to_string();
    }
    let staged = snapshot.status.staged.len();
    let unstaged = snapshot.status.unstaged.len();
    let untracked = snapshot.status.untracked.len();
    if staged == 0 && unstaged == 0 && untracked == 0 {
        return "Clean".to_string();
    }
    let mut parts = Vec::new();
    if staged > 0 {
        parts.push(format!("{staged} staged"));
    }
    if unstaged > 0 {
        parts.push(format!("{unstaged} unstaged"));
    }
    if untracked > 0 {
        parts.push(format!("{untracked} untracked"));
    }
    parts.join(" | ")
}

fn stageable_paths(snapshot: &StoreSnapshot) -> Vec<String> {
    snapshot
        .status
        .unstaged
        .iter()
        .chain(snapshot.status.untracked.iter())
        .cloned()
        .collect()
}

fn selected_file_group(snapshot: &StoreSnapshot, file: &str) -> Option<StatusFileGroup> {
    if snapshot.status.staged.iter().any(|path| path == file) {
        Some(StatusFileGroup::Staged)
    } else if snapshot.status.unstaged.iter().any(|path| path == file) {
        Some(StatusFileGroup::Unstaged)
    } else if snapshot.status.untracked.iter().any(|path| path == file) {
        Some(StatusFileGroup::Untracked)
    } else {
        None
    }
}

fn commit_disabled_reason(
    snapshot: &StoreSnapshot,
    commit_summary: &str,
    busy: bool,
) -> Option<&'static str> {
    if busy {
        Some("Repository operation is running")
    } else if snapshot.status.staged.is_empty() {
        Some("No staged changes")
    } else if commit_summary.trim().is_empty() {
        Some("Commit message is empty")
    } else {
        None
    }
}

fn commit_button_label(staged_count: usize) -> String {
    match staged_count {
        1 => "Commit 1 file".to_string(),
        count => format!("Commit {count} files"),
    }
}

fn diff_header_summary(snapshot: &StoreSnapshot) -> String {
    match snapshot.diff.source.as_ref() {
        Some(DiffSource::Worktree { paths }) | Some(DiffSource::Index { paths }) => paths
            .first()
            .cloned()
            .or_else(|| snapshot.diff.descriptor.as_ref()?.file_path.clone())
            .unwrap_or_else(|| "Selected file".to_string()),
        Some(DiffSource::Commit { oid }) => snapshot
            .commit_cache
            .get(oid)
            .map(|details| format!("{} - {}", short_oid(oid), first_line(&details.message)))
            .unwrap_or_else(|| format!("Commit {}", short_oid(oid))),
        Some(DiffSource::Compare { base, head }) => format!("{base}..{head}"),
        None => "No diff selected".to_string(),
    }
}

fn first_line(value: &str) -> &str {
    value.lines().next().unwrap_or(value)
}

fn bool_word(value: bool) -> &'static str {
    if value { "available" } else { "not available" }
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct BottomStatusMessage {
    text: String,
    is_error: bool,
}

fn bottom_status_message(
    state: &RuntimeAdapterState,
    ui_error: Option<&str>,
) -> BottomStatusMessage {
    if state.busy {
        return BottomStatusMessage {
            text: format!(
                "Running: {}",
                state
                    .current_operation
                    .as_deref()
                    .unwrap_or("repository operation")
            ),
            is_error: false,
        };
    }
    if let Some(error) = state.last_error.as_ref() {
        return BottomStatusMessage {
            text: error.to_string(),
            is_error: true,
        };
    }
    let stale_empty_repo_error = ui_error.is_some_and(|message| {
        message == "Repository path is empty." && state.snapshot.repo.is_some()
    });
    if let Some(error) = ui_error.filter(|message| !message.trim().is_empty())
        && !stale_empty_repo_error
    {
        return BottomStatusMessage {
            text: error.to_string(),
            is_error: true,
        };
    }
    if let Some(message) = state.last_message.as_deref() {
        return BottomStatusMessage {
            text: message.to_string(),
            is_error: false,
        };
    }
    BottomStatusMessage {
        text: "Ready".to_string(),
        is_error: false,
    }
}

fn bottom_repo_context_label(snapshot: &StoreSnapshot) -> String {
    match snapshot.repo.as_ref() {
        Some(repo) => {
            let branch = repo.head.as_deref().unwrap_or("detached HEAD");
            format!("Repo: {} | Branch: {branch}", repo_display_name(&repo.root))
        }
        None => "Repo <not opened>".to_string(),
    }
}

fn render_panel_notice(ui: &mut egui::Ui, panel: PanelId) {
    match panel.panel_status() {
        PanelStatus::Core => {}
        PanelStatus::Preview => {
            ui.colored_label(Color32::from_rgb(251, 191, 36), "Preview");
            ui.weak(preview_panel_copy(panel));
        }
        PanelStatus::Advanced => {
            ui.colored_label(Color32::from_rgb(251, 191, 36), "Advanced");
            ui.weak(advanced_panel_copy(panel));
        }
        PanelStatus::Developer => {
            ui.colored_label(Color32::from_rgb(125, 211, 252), "Developer");
            ui.weak("Runtime context, action availability, plugin health, and state diagnostics.");
        }
        PanelStatus::DeveloperPreview => {
            ui.colored_label(Color32::from_rgb(251, 191, 36), "Preview");
            ui.weak("This developer-mode panel is a recovery and safety preview.");
        }
    }
}

fn preview_panel_copy(panel: PanelId) -> &'static str {
    match panel {
        PanelId::Tags => {
            "Local tag list and basic tag actions are available; detached-head safety is still being polished."
        }
        PanelId::Compare => {
            "Ref comparison and diff preview are available; production compare workflow is still incomplete."
        }
        PanelId::PullRequests => "Provider integration is not complete yet.",
        _ => "This panel is available as a preview and may not cover the full workflow yet.",
    }
}

fn advanced_panel_copy(panel: PanelId) -> &'static str {
    match panel {
        PanelId::Workspaces => "Multi-repo workspace orchestration is experimental.",
        PanelId::PullRequests => {
            "Provider-backed pull request workflows depend on remotes and credentials."
        }
        PanelId::BranchStacks => {
            "Stacked branch and virtual branch workflows are research surfaces."
        }
        PanelId::Stash => "Stash operations are available as advanced recovery tools.",
        PanelId::Worktrees => "Worktree operations are available as advanced repository tools.",
        PanelId::Submodules => "Submodule operations are available as advanced repository tools.",
        PanelId::Conflicts => {
            "Conflict workflows are an advanced safety surface, not the full resolver."
        }
        _ => "This panel is hidden until Advanced mode is enabled.",
    }
}

fn panel_for_title(title: &str) -> Option<PanelId> {
    match title {
        "Stash" => Some(PanelId::Stash),
        "Worktrees" => Some(PanelId::Worktrees),
        "Submodules" => Some(PanelId::Submodules),
        _ => None,
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

fn render_operation_preview(ui: &mut egui::Ui, preview: &OperationPreview) {
    ui.label(RichText::new(preview.summary.as_str()).strong());
    ui.label(format!(
        "Operation: {} | danger: {}",
        preview.operation,
        format_danger(&preview.danger)
    ));
    ui.separator();
    ui.columns(2, |columns| {
        columns[0].label(RichText::new("Worktree").strong());
        columns[0].label(format_impact(&preview.worktree_impact));
        columns[0].label(RichText::new("Index").strong());
        columns[0].label(format_impact(&preview.index_impact));

        columns[1].label(RichText::new("Git commands").strong());
        for command in &preview.git_commands {
            columns[1].label(RichText::new(command).monospace());
        }
    });

    if !preview.affected_refs.is_empty() {
        ui.separator();
        ui.label(RichText::new("Affected Refs").strong());
        for item in &preview.affected_refs {
            ui.label(format!(
                "{}: {} -> {} ({})",
                item.name,
                item.before.as_deref().unwrap_or("<none>"),
                item.after.as_deref().unwrap_or("<none>"),
                item.impact
            ));
        }
    }

    if !preview.affected_files.is_empty() {
        ui.separator();
        ui.label(RichText::new("Affected Files").strong());
        for item in &preview.affected_files {
            ui.label(format!(
                "{}: {}{}",
                item.path,
                item.impact,
                item.detail
                    .as_deref()
                    .map(|detail| format!(" ({detail})"))
                    .unwrap_or_default()
            ));
        }
    }

    if !preview.commits_rewritten.is_empty() {
        ui.separator();
        ui.label(RichText::new("Commits Rewritten").strong());
        for commit in &preview.commits_rewritten {
            ui.label(format!(
                "{} {} ({})",
                short_oid(&commit.oid),
                commit.summary,
                commit.action
            ));
        }
    }

    if !preview.warnings.is_empty() {
        ui.separator();
        ui.label(RichText::new("Warnings").strong());
        for warning in &preview.warnings {
            ui.colored_label(warning_color(&warning.level), warning.message.as_str());
        }
    }

    if let Some(recommended) = preview.recommended_action.as_deref() {
        ui.separator();
        ui.label(RichText::new("Recommended").strong());
        ui.label(recommended);
    }
}

fn format_impact(impact: &state_store::ImpactSummary) -> String {
    let level = match impact.level {
        ImpactLevel::None => "none",
        ImpactLevel::Read => "read",
        ImpactLevel::Write => "write",
        ImpactLevel::Destructive => "destructive",
    };
    format!("{level}: {}", impact.summary)
}

fn warning_color(level: &PreviewWarningLevel) -> Color32 {
    match level {
        PreviewWarningLevel::Info => Color32::from_rgb(125, 211, 252),
        PreviewWarningLevel::Warning => Color32::from_rgb(251, 191, 36),
        PreviewWarningLevel::Danger => Color32::from_rgb(248, 113, 113),
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

fn status_is_dirty(snapshot: &StoreSnapshot) -> bool {
    !snapshot.status.staged.is_empty()
        || !snapshot.status.unstaged.is_empty()
        || !snapshot.status.untracked.is_empty()
}

fn branch_name_input_is_valid(name: &str) -> bool {
    !name.is_empty()
        && !name.starts_with('-')
        && !name.starts_with('/')
        && !name.ends_with('/')
        && !name.contains("//")
        && !name.contains("..")
        && !name.contains("@{")
        && !name.contains('\\')
        && !name.contains(' ')
        && !name.contains('~')
        && !name.contains('^')
        && !name.contains(':')
        && !name.contains('?')
        && !name.contains('*')
        && !name.contains('[')
        && !name.ends_with(".lock")
        && !name
            .split('/')
            .any(|part| part.is_empty() || part.ends_with('.'))
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
                        ui.label(diff_line_text(line, ui.visuals().dark_mode));
                        ui.label(diff_line_text(line, ui.visuals().dark_mode));
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
        ui.label(diff_line_text(line, ui.visuals().dark_mode));
        return;
    };
    let Some(selected_lines) = selected_changed_lines.as_mut() else {
        ui.label(diff_line_text(line, ui.visuals().dark_mode));
        return;
    };
    let selected_lines = &mut **selected_lines;
    let selected = selected_lines.contains(&changed_index);
    if ui
        .selectable_label(selected, diff_line_text(line, ui.visuals().dark_mode))
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
        ui.label(diff_line_text(line, ui.visuals().dark_mode));
    }
}

fn diff_line_text(line: &str, dark: bool) -> RichText {
    let (color, background) = if is_conflict_marker_line(line) {
        (
            design_tokens::warning(dark),
            design_tokens::surface_alt(dark),
        )
    } else if line.starts_with('+') {
        (
            design_tokens::success(dark),
            Color32::from_rgba_unmultiplied(31, 157, 85, 38),
        )
    } else if line.starts_with('-') {
        (
            design_tokens::danger(dark),
            Color32::from_rgba_unmultiplied(214, 69, 69, 42),
        )
    } else if line.starts_with("@@") {
        (
            design_tokens::accent(dark),
            design_tokens::accent_soft(dark),
        )
    } else {
        (design_tokens::text(dark), design_tokens::surface(dark))
    };
    RichText::new(line.to_string())
        .monospace()
        .color(color)
        .background_color(background)
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
    ui.horizontal(|ui| {
        ui.colored_label(journal_status_color(entry), format!("{:?}", entry.status));
        ui.label(format!("#{} {}", entry.id, entry.op));
        if let Some(job_id) = entry.job_id {
            ui.label(format!("job {job_id}"));
        }
        if let Some(risk) = entry.risk.as_ref() {
            ui.label(format!("risk {}", format_danger(risk)));
        }
    });
    ui.label(format!(
        "repo: {}",
        entry.repo_root.as_deref().unwrap_or("<unknown>")
    ));
    if !entry.params.is_empty() {
        ui.label(format!("params: {}", entry.params.join(" ")));
    }
    if let Some(error) = entry.error.as_deref() {
        ui.colored_label(Color32::from_rgb(248, 113, 113), error);
    }
    if let Some(pre_refs) = entry.pre_refs.as_ref() {
        ui.label(format!(
            "pre refs: head={} oid={} branches={} tags={} tracked_refs={}",
            pre_refs.head.as_deref().unwrap_or("<none>"),
            pre_refs.head_oid.as_deref().unwrap_or("<unknown>"),
            pre_refs.branch_count,
            pre_refs.tag_count,
            pre_refs.refs.len()
        ));
    }
    if let Some(post_refs) = entry.post_refs.as_ref() {
        ui.label(format!(
            "post refs: head={} oid={} branches={} tags={} tracked_refs={}",
            post_refs.head.as_deref().unwrap_or("<none>"),
            post_refs.head_oid.as_deref().unwrap_or("<unknown>"),
            post_refs.branch_count,
            post_refs.tag_count,
            post_refs.refs.len()
        ));
    }
    if !entry.backup_refs.is_empty() {
        ui.label(RichText::new("Recovery").strong());
        for backup in &entry.backup_refs {
            ui.label(format!(
                "{} -> {} ({})",
                backup.name,
                short_oid(&backup.target_oid),
                backup.target_ref
            ));
        }
    }
}

fn journal_status_color(entry: &OperationJournalEntry) -> Color32 {
    match entry.status {
        JournalStatus::Started => Color32::from_rgb(125, 211, 252),
        JournalStatus::Succeeded => Color32::from_rgb(74, 222, 128),
        JournalStatus::Failed => Color32::from_rgb(248, 113, 113),
    }
}

fn recovery_branch_name(raw: &str, entry_id: u64) -> String {
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        format!("branchforge/recovery/{entry_id}")
    } else if entry_id == 0 {
        trimmed.to_string()
    } else if trimmed.ends_with('/') {
        format!("{trimmed}{entry_id}")
    } else {
        trimmed.to_string()
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
    fn bottom_status_uses_open_repo_context_when_previous_ui_error_exists() {
        let state = RuntimeAdapterState {
            snapshot: StoreSnapshot {
                repo: Some(plugin_api::RepoSnapshot {
                    root: "/tmp/branchforge-demo".to_string(),
                    head: Some("main".to_string()),
                    conflict_state: None,
                }),
                version: 7,
                ..StoreSnapshot::default()
            },
            last_message: Some("opened repository /tmp/branchforge-demo".to_string()),
            ..RuntimeAdapterState::default()
        };

        let message = bottom_status_message(&state, Some("Repository path is empty."));
        assert_eq!(message.text, "opened repository /tmp/branchforge-demo");
        assert!(!message.is_error);
        assert_eq!(
            bottom_repo_context_label(&state.snapshot),
            "Repo: branchforge-demo | Branch: main"
        );
    }

    #[test]
    fn advanced_mode_controls_sidebar_visibility() {
        assert!(PanelId::Status.is_visible(false, false));
        assert!(PanelId::Remotes.is_visible(false, false));
        assert!(!PanelId::Workspaces.is_visible(false, false));
        assert!(PanelId::Workspaces.is_visible(true, false));
        assert!(PanelId::Workspaces.is_visible(false, true));
        assert!(!PanelId::Diagnostics.is_visible(true, false));
        assert!(PanelId::Diagnostics.is_visible(false, true));
        assert!(!PanelId::Journal.is_visible(true, false));
        assert!(PanelId::Journal.is_visible(false, true));
        assert_eq!(PanelId::History.label(), "History");
    }

    #[test]
    fn branch_name_input_validation_blocks_obvious_invalid_refs() {
        assert!(branch_name_input_is_valid("feature/daily-driver"));
        assert!(!branch_name_input_is_valid(""));
        assert!(!branch_name_input_is_valid("bad name"));
        assert!(!branch_name_input_is_valid("bad..name"));
        assert!(!branch_name_input_is_valid("topic.lock"));
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
