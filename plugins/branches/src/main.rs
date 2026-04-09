use plugin_api::{
    ActionEffects, ActionSpec, ConfirmPolicy, DangerLevel, PluginHello, PluginRegister, ViewSpec,
    serve_static_plugin,
};

fn spec(
    action_id: &str,
    title: &str,
    danger: Option<DangerLevel>,
    effects: ActionEffects,
    confirm_policy: ConfirmPolicy,
) -> ActionSpec {
    ActionSpec {
        action_id: action_id.to_string(),
        title: title.to_string(),
        when: Some("repo.is_open".to_string()),
        params_schema: None,
        danger,
        effects,
        confirm_policy,
    }
}

fn build_register_payload() -> PluginRegister {
    PluginRegister {
        actions: vec![
            spec(
                "branch.checkout",
                "Checkout Branch",
                None,
                ActionEffects {
                    writes_refs: true,
                    writes_worktree: true,
                    danger_level: DangerLevel::Medium,
                    ..ActionEffects::default()
                },
                ConfirmPolicy::OnDanger,
            ),
            spec(
                "branch.create",
                "Create Branch",
                None,
                ActionEffects {
                    writes_refs: true,
                    danger_level: DangerLevel::Medium,
                    ..ActionEffects::default()
                },
                ConfirmPolicy::OnDanger,
            ),
            spec(
                "branch.rename",
                "Rename Branch",
                None,
                ActionEffects {
                    writes_refs: true,
                    danger_level: DangerLevel::Medium,
                    ..ActionEffects::default()
                },
                ConfirmPolicy::OnDanger,
            ),
            spec(
                "branch.delete",
                "Delete Branch",
                Some(DangerLevel::High),
                ActionEffects::mutating_refs(),
                ConfirmPolicy::Always,
            ),
            spec(
                "rebase.interactive",
                "Interactive Rebase",
                Some(DangerLevel::High),
                ActionEffects {
                    writes_refs: true,
                    writes_index: true,
                    writes_worktree: true,
                    danger_level: DangerLevel::High,
                    ..ActionEffects::default()
                },
                ConfirmPolicy::Always,
            ),
            spec(
                "rebase.plan.create",
                "Create Rebase Plan",
                Some(DangerLevel::Medium),
                ActionEffects::read_only(),
                ConfirmPolicy::OnDanger,
            ),
            spec(
                "rebase.plan.set_action",
                "Set Rebase Plan Action",
                Some(DangerLevel::Low),
                ActionEffects::read_only(),
                ConfirmPolicy::Never,
            ),
            spec(
                "rebase.plan.move",
                "Reorder Rebase Plan Entry",
                Some(DangerLevel::Low),
                ActionEffects::read_only(),
                ConfirmPolicy::Never,
            ),
            spec(
                "rebase.plan.clear",
                "Clear Rebase Plan",
                Some(DangerLevel::Low),
                ActionEffects::read_only(),
                ConfirmPolicy::Never,
            ),
            spec(
                "rebase.execute",
                "Execute Rebase Plan",
                Some(DangerLevel::High),
                ActionEffects {
                    writes_refs: true,
                    writes_index: true,
                    writes_worktree: true,
                    danger_level: DangerLevel::High,
                    ..ActionEffects::default()
                },
                ConfirmPolicy::Always,
            ),
            spec(
                "rebase.continue",
                "Continue Rebase",
                Some(DangerLevel::Medium),
                ActionEffects {
                    writes_refs: true,
                    writes_index: true,
                    writes_worktree: true,
                    danger_level: DangerLevel::Medium,
                    ..ActionEffects::default()
                },
                ConfirmPolicy::OnDanger,
            ),
            spec(
                "rebase.skip",
                "Skip Rebase Commit",
                Some(DangerLevel::Medium),
                ActionEffects {
                    writes_refs: true,
                    writes_index: true,
                    writes_worktree: true,
                    danger_level: DangerLevel::Medium,
                    ..ActionEffects::default()
                },
                ConfirmPolicy::OnDanger,
            ),
            spec(
                "rebase.abort",
                "Abort Rebase",
                Some(DangerLevel::Medium),
                ActionEffects {
                    writes_refs: true,
                    writes_index: true,
                    writes_worktree: true,
                    danger_level: DangerLevel::Medium,
                    ..ActionEffects::default()
                },
                ConfirmPolicy::OnDanger,
            ),
            spec(
                "merge.execute",
                "Merge Branch",
                Some(DangerLevel::High),
                ActionEffects {
                    writes_refs: true,
                    writes_index: true,
                    writes_worktree: true,
                    danger_level: DangerLevel::High,
                    ..ActionEffects::default()
                },
                ConfirmPolicy::Always,
            ),
            spec(
                "merge.abort",
                "Abort Merge",
                Some(DangerLevel::Medium),
                ActionEffects {
                    writes_refs: true,
                    writes_index: true,
                    writes_worktree: true,
                    danger_level: DangerLevel::Medium,
                    ..ActionEffects::default()
                },
                ConfirmPolicy::OnDanger,
            ),
            spec(
                "conflict.list",
                "List Conflicts",
                Some(DangerLevel::Low),
                ActionEffects::read_only(),
                ConfirmPolicy::Never,
            ),
            spec(
                "conflict.focus",
                "Focus Conflict File",
                Some(DangerLevel::Low),
                ActionEffects::read_only(),
                ConfirmPolicy::Never,
            ),
            spec(
                "conflict.resolve.ours",
                "Resolve Conflict (Ours)",
                Some(DangerLevel::Medium),
                ActionEffects::mutating_worktree(),
                ConfirmPolicy::OnDanger,
            ),
            spec(
                "conflict.resolve.theirs",
                "Resolve Conflict (Theirs)",
                Some(DangerLevel::Medium),
                ActionEffects::mutating_worktree(),
                ConfirmPolicy::OnDanger,
            ),
            spec(
                "conflict.mark_resolved",
                "Mark Conflict Resolved",
                Some(DangerLevel::Medium),
                ActionEffects {
                    writes_index: true,
                    danger_level: DangerLevel::Medium,
                    ..ActionEffects::default()
                },
                ConfirmPolicy::OnDanger,
            ),
            spec(
                "conflict.continue",
                "Continue Conflict Session",
                Some(DangerLevel::Medium),
                ActionEffects {
                    writes_refs: true,
                    writes_index: true,
                    writes_worktree: true,
                    danger_level: DangerLevel::Medium,
                    ..ActionEffects::default()
                },
                ConfirmPolicy::OnDanger,
            ),
            spec(
                "conflict.abort",
                "Abort Conflict Session",
                Some(DangerLevel::Medium),
                ActionEffects {
                    writes_refs: true,
                    writes_index: true,
                    writes_worktree: true,
                    danger_level: DangerLevel::Medium,
                    ..ActionEffects::default()
                },
                ConfirmPolicy::OnDanger,
            ),
            spec(
                "reset.soft",
                "Reset --soft",
                Some(DangerLevel::Medium),
                ActionEffects {
                    writes_refs: true,
                    danger_level: DangerLevel::Medium,
                    ..ActionEffects::default()
                },
                ConfirmPolicy::OnDanger,
            ),
            spec(
                "reset.mixed",
                "Reset --mixed",
                Some(DangerLevel::High),
                ActionEffects {
                    writes_refs: true,
                    writes_index: true,
                    danger_level: DangerLevel::High,
                    ..ActionEffects::default()
                },
                ConfirmPolicy::Always,
            ),
            spec(
                "reset.hard",
                "Reset --hard",
                Some(DangerLevel::High),
                ActionEffects {
                    writes_refs: true,
                    writes_index: true,
                    writes_worktree: true,
                    danger_level: DangerLevel::High,
                    ..ActionEffects::default()
                },
                ConfirmPolicy::Always,
            ),
        ],
        views: vec![ViewSpec {
            view_id: "branches.panel".to_string(),
            title: "Branches".to_string(),
            slot: "left".to_string(),
            when: Some("repo.is_open".to_string()),
        }],
    }
}

fn main() {
    let hello = PluginHello {
        plugin_id: "branches".to_string(),
        version: env!("CARGO_PKG_VERSION").to_string(),
    };
    let register = build_register_payload();

    if let Err(err) = serve_static_plugin(hello, register, |action_id, context| {
        serde_json::json!({
            "ok": true,
            "plugin_id": "branches",
            "action_id": action_id,
            "selection_files": context.selection_files,
        })
    }) {
        eprintln!("branches runtime failed: {err:?}");
        std::process::exit(1);
    }
}
