#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PanelId {
    Status,
    History,
    Diff,
    Branches,
    Tags,
    Compare,
    Stash,
    Worktrees,
    Submodules,
    Conflicts,
    Diagnostics,
    Journal,
}

impl PanelId {
    pub const ALL: [PanelId; 12] = [
        PanelId::Status,
        PanelId::History,
        PanelId::Diff,
        PanelId::Branches,
        PanelId::Tags,
        PanelId::Compare,
        PanelId::Stash,
        PanelId::Worktrees,
        PanelId::Submodules,
        PanelId::Conflicts,
        PanelId::Diagnostics,
        PanelId::Journal,
    ];

    pub fn label(self) -> &'static str {
        match self {
            Self::Status => "Status",
            Self::History => "History Graph",
            Self::Diff => "Diff",
            Self::Branches => "Branches",
            Self::Tags => "Tags",
            Self::Compare => "Compare",
            Self::Stash => "Stash",
            Self::Worktrees => "Worktrees",
            Self::Submodules => "Submodules",
            Self::Conflicts => "Conflicts",
            Self::Diagnostics => "Diagnostics",
            Self::Journal => "Journal",
        }
    }

    pub fn host_panel(self) -> Option<&'static str> {
        match self {
            Self::Status => Some("status"),
            Self::History => Some("history"),
            Self::Branches => Some("branches"),
            Self::Tags => Some("tags"),
            Self::Compare => Some("compare"),
            Self::Diagnostics => Some("diagnostics"),
            Self::Diff
            | Self::Stash
            | Self::Worktrees
            | Self::Submodules
            | Self::Conflicts
            | Self::Journal => None,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LayoutState {
    pub left_sidebar_open: bool,
    pub right_inspector_open: bool,
    pub dark_mode: bool,
    pub advanced_mode: bool,
}

impl Default for LayoutState {
    fn default() -> Self {
        Self {
            left_sidebar_open: true,
            right_inspector_open: true,
            dark_mode: true,
            advanced_mode: false,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct CommandPaletteState {
    pub open: bool,
    pub filter: String,
    pub args: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct DiffViewState {
    pub side_by_side: bool,
    pub selected_hunk: Option<usize>,
    pub selected_changed_lines: Vec<usize>,
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct GraphViewState {
    pub selected_row: Option<usize>,
    pub search: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct ConflictViewState {
    pub selected_path: Option<String>,
    pub unresolved_count: usize,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ConfirmationDialog {
    pub action_id: String,
    pub args: Vec<String>,
    pub title: String,
    pub message: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PreviewDialog {
    pub action_id: String,
    pub args: Vec<String>,
    pub title: String,
    pub preview: state_store::OperationPreview,
    pub understood: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct JournalViewState {
    pub selected_entry_id: Option<u64>,
    pub filter: String,
    pub recovery_only: bool,
    pub reflog_reference: String,
    pub recovery_branch_name: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DesktopUiState {
    pub active_panel: PanelId,
    pub layout: LayoutState,
    pub command_palette: CommandPaletteState,
    pub selected_sidebar_item: PanelId,
    pub diff_view_state: DiffViewState,
    pub graph_view_state: GraphViewState,
    pub conflict_view_state: ConflictViewState,
    pub journal_view_state: JournalViewState,
    pub pending_confirmation: Option<ConfirmationDialog>,
    pub pending_preview: Option<PreviewDialog>,
}

impl Default for DesktopUiState {
    fn default() -> Self {
        Self {
            active_panel: PanelId::Status,
            layout: LayoutState::default(),
            command_palette: CommandPaletteState::default(),
            selected_sidebar_item: PanelId::Status,
            diff_view_state: DiffViewState::default(),
            graph_view_state: GraphViewState::default(),
            conflict_view_state: ConflictViewState::default(),
            journal_view_state: JournalViewState {
                reflog_reference: "HEAD".to_string(),
                recovery_branch_name: "branchforge/recovery".to_string(),
                ..JournalViewState::default()
            },
            pending_confirmation: None,
            pending_preview: None,
        }
    }
}
