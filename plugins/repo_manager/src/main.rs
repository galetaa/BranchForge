use plugin_api::{
    ActionEffects, ActionSpec, ConfirmPolicy, DangerLevel, PluginHello, PluginRegister,
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
            ActionSpec {
                action_id: "repo.open".to_string(),
                title: "Open Repository".to_string(),
                when: Some("always".to_string()),
                params_schema: None,
                danger: None,
                effects: ActionEffects::read_only(),
                confirm_policy: ConfirmPolicy::Never,
            },
            ActionSpec {
                action_id: "worktree.list".to_string(),
                title: "List Worktrees".to_string(),
                when: Some("repo.is_open".to_string()),
                params_schema: None,
                danger: None,
                effects: ActionEffects::read_only(),
                confirm_policy: ConfirmPolicy::Never,
            },
            ActionSpec {
                action_id: "worktree.create".to_string(),
                title: "Create Worktree".to_string(),
                when: Some("repo.is_open".to_string()),
                params_schema: None,
                danger: Some(DangerLevel::Medium),
                effects: ActionEffects {
                    writes_refs: true,
                    writes_worktree: true,
                    danger_level: DangerLevel::Medium,
                    ..ActionEffects::default()
                },
                confirm_policy: ConfirmPolicy::OnDanger,
            },
            ActionSpec {
                action_id: "worktree.remove".to_string(),
                title: "Remove Worktree".to_string(),
                when: Some("repo.is_open".to_string()),
                params_schema: None,
                danger: Some(DangerLevel::High),
                effects: ActionEffects {
                    writes_refs: true,
                    writes_worktree: true,
                    danger_level: DangerLevel::High,
                    ..ActionEffects::default()
                },
                confirm_policy: ConfirmPolicy::Always,
            },
            ActionSpec {
                action_id: "worktree.open".to_string(),
                title: "Open Worktree".to_string(),
                when: Some("repo.is_open".to_string()),
                params_schema: None,
                danger: None,
                effects: ActionEffects::read_only(),
                confirm_policy: ConfirmPolicy::Never,
            },
            ActionSpec {
                action_id: "submodule.list".to_string(),
                title: "List Submodules".to_string(),
                when: Some("repo.is_open".to_string()),
                params_schema: None,
                danger: None,
                effects: ActionEffects::read_only(),
                confirm_policy: ConfirmPolicy::Never,
            },
            spec(
                "submodule.init_update",
                "Init/Update Submodule",
                Some(DangerLevel::Medium),
                ActionEffects {
                    writes_worktree: true,
                    danger_level: DangerLevel::Medium,
                    ..ActionEffects::default()
                },
                ConfirmPolicy::OnDanger,
            ),
            ActionSpec {
                action_id: "submodule.open".to_string(),
                title: "Open Submodule".to_string(),
                when: Some("repo.is_open".to_string()),
                params_schema: None,
                danger: None,
                effects: ActionEffects::read_only(),
                confirm_policy: ConfirmPolicy::Never,
            },
        ],
        views: Vec::new(),
    }
}

fn main() {
    let hello = PluginHello {
        plugin_id: "repo_manager".to_string(),
        version: env!("CARGO_PKG_VERSION").to_string(),
    };
    let register = build_register_payload();

    if let Err(err) = serve_static_plugin(hello, register, |action_id, context| {
        serde_json::json!({
            "ok": true,
            "plugin_id": "repo_manager",
            "action_id": action_id,
            "selection_files": context.selection_files,
        })
    }) {
        eprintln!("repo_manager runtime failed: {err:?}");
        std::process::exit(1);
    }
}
