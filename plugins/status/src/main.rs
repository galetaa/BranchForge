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
                "index.stage_selected",
                "Stage Selected",
                None,
                ActionEffects {
                    writes_index: true,
                    danger_level: DangerLevel::Low,
                    ..ActionEffects::default()
                },
                ConfirmPolicy::Never,
            ),
            spec(
                "index.unstage_selected",
                "Unstage Selected",
                None,
                ActionEffects {
                    writes_index: true,
                    danger_level: DangerLevel::Low,
                    ..ActionEffects::default()
                },
                ConfirmPolicy::Never,
            ),
            spec(
                "index.stage_hunk",
                "Stage Hunk",
                None,
                ActionEffects {
                    writes_index: true,
                    danger_level: DangerLevel::Low,
                    ..ActionEffects::default()
                },
                ConfirmPolicy::Never,
            ),
            spec(
                "index.stage_lines",
                "Stage Lines",
                None,
                ActionEffects {
                    writes_index: true,
                    danger_level: DangerLevel::Low,
                    ..ActionEffects::default()
                },
                ConfirmPolicy::Never,
            ),
            spec(
                "index.unstage_hunk",
                "Unstage Hunk",
                None,
                ActionEffects {
                    writes_index: true,
                    danger_level: DangerLevel::Low,
                    ..ActionEffects::default()
                },
                ConfirmPolicy::Never,
            ),
            spec(
                "index.unstage_lines",
                "Unstage Lines",
                None,
                ActionEffects {
                    writes_index: true,
                    danger_level: DangerLevel::Low,
                    ..ActionEffects::default()
                },
                ConfirmPolicy::Never,
            ),
            spec(
                "commit.create",
                "Commit",
                None,
                ActionEffects {
                    writes_refs: true,
                    writes_index: true,
                    danger_level: DangerLevel::Medium,
                    ..ActionEffects::default()
                },
                ConfirmPolicy::OnDanger,
            ),
            spec(
                "commit.amend",
                "Amend Commit",
                Some(DangerLevel::Medium),
                ActionEffects {
                    writes_refs: true,
                    writes_index: true,
                    danger_level: DangerLevel::Medium,
                    ..ActionEffects::default()
                },
                ConfirmPolicy::OnDanger,
            ),
            spec(
                "file.discard",
                "Discard File Changes",
                Some(DangerLevel::High),
                ActionEffects {
                    writes_worktree: true,
                    danger_level: DangerLevel::High,
                    ..ActionEffects::default()
                },
                ConfirmPolicy::Always,
            ),
            spec(
                "file.discard_hunk",
                "Discard Hunk",
                Some(DangerLevel::High),
                ActionEffects {
                    writes_worktree: true,
                    danger_level: DangerLevel::High,
                    ..ActionEffects::default()
                },
                ConfirmPolicy::Always,
            ),
            spec(
                "file.discard_lines",
                "Discard Lines",
                Some(DangerLevel::High),
                ActionEffects {
                    writes_worktree: true,
                    danger_level: DangerLevel::High,
                    ..ActionEffects::default()
                },
                ConfirmPolicy::Always,
            ),
            spec(
                "stash.create",
                "Create Stash",
                Some(DangerLevel::Medium),
                ActionEffects {
                    writes_index: true,
                    writes_worktree: true,
                    danger_level: DangerLevel::Medium,
                    ..ActionEffects::default()
                },
                ConfirmPolicy::OnDanger,
            ),
            spec(
                "stash.list",
                "List Stashes",
                None,
                ActionEffects::read_only(),
                ConfirmPolicy::Never,
            ),
            spec(
                "stash.apply",
                "Apply Stash",
                Some(DangerLevel::Medium),
                ActionEffects {
                    writes_index: true,
                    writes_worktree: true,
                    danger_level: DangerLevel::Medium,
                    ..ActionEffects::default()
                },
                ConfirmPolicy::OnDanger,
            ),
            spec(
                "stash.pop",
                "Pop Stash",
                Some(DangerLevel::Medium),
                ActionEffects {
                    writes_index: true,
                    writes_worktree: true,
                    danger_level: DangerLevel::Medium,
                    ..ActionEffects::default()
                },
                ConfirmPolicy::OnDanger,
            ),
            spec(
                "stash.drop",
                "Drop Stash",
                Some(DangerLevel::Medium),
                ActionEffects {
                    writes_refs: true,
                    danger_level: DangerLevel::Medium,
                    ..ActionEffects::default()
                },
                ConfirmPolicy::OnDanger,
            ),
        ],
        views: vec![ViewSpec {
            view_id: "status.panel".to_string(),
            title: "Status".to_string(),
            slot: "left".to_string(),
            when: Some("repo.is_open".to_string()),
        }],
    }
}

fn main() {
    let hello = PluginHello {
        plugin_id: "status".to_string(),
        version: env!("CARGO_PKG_VERSION").to_string(),
    };
    let register = build_register_payload();

    if let Err(err) = serve_static_plugin(hello, register, |action_id, context| {
        serde_json::json!({
            "ok": true,
            "plugin_id": "status",
            "action_id": action_id,
            "selection_files": context.selection_files,
        })
    }) {
        eprintln!("status runtime failed: {err:?}");
        std::process::exit(1);
    }
}
