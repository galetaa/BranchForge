use plugin_api::{
    ActionEffects, ActionSpec, ConfirmPolicy, DangerLevel, PluginHello, PluginRegister, ViewSpec,
    serve_static_plugin,
};

fn build_register_payload() -> PluginRegister {
    PluginRegister {
        actions: vec![
            ActionSpec {
                action_id: "diagnostics.journal_summary".to_string(),
                title: "Show Journal Summary".to_string(),
                when: Some("repo.is_open".to_string()),
                params_schema: None,
                danger: None,
                effects: ActionEffects::read_only(),
                confirm_policy: ConfirmPolicy::Never,
            },
            ActionSpec {
                action_id: "diagnostics.repo_capabilities".to_string(),
                title: "Show Repo Capabilities".to_string(),
                when: Some("repo.is_open".to_string()),
                params_schema: None,
                danger: None,
                effects: ActionEffects::read_only(),
                confirm_policy: ConfirmPolicy::Never,
            },
            ActionSpec {
                action_id: "diagnostics.lfs_status".to_string(),
                title: "Show LFS Status".to_string(),
                when: Some("repo.is_open".to_string()),
                params_schema: None,
                danger: None,
                effects: ActionEffects::read_only(),
                confirm_policy: ConfirmPolicy::Never,
            },
            ActionSpec {
                action_id: "diagnostics.lfs_fetch".to_string(),
                title: "Fetch LFS Objects".to_string(),
                when: Some("repo.is_open".to_string()),
                params_schema: None,
                danger: Some(DangerLevel::Low),
                effects: ActionEffects {
                    network: true,
                    danger_level: DangerLevel::Low,
                    ..ActionEffects::default()
                },
                confirm_policy: ConfirmPolicy::Never,
            },
            ActionSpec {
                action_id: "diagnostics.lfs_pull".to_string(),
                title: "Pull LFS Objects".to_string(),
                when: Some("repo.is_open".to_string()),
                params_schema: None,
                danger: Some(DangerLevel::Low),
                effects: ActionEffects {
                    network: true,
                    danger_level: DangerLevel::Low,
                    ..ActionEffects::default()
                },
                confirm_policy: ConfirmPolicy::Never,
            },
        ],
        views: vec![ViewSpec {
            view_id: "diagnostics.panel".to_string(),
            title: "Diagnostics".to_string(),
            slot: "right".to_string(),
            when: Some("repo.is_open".to_string()),
        }],
    }
}

fn main() {
    let hello = PluginHello {
        plugin_id: "diagnostics".to_string(),
        version: env!("CARGO_PKG_VERSION").to_string(),
    };
    let register = build_register_payload();

    if let Err(err) = serve_static_plugin(hello, register, |action_id, context| {
        serde_json::json!({
            "ok": true,
            "plugin_id": "diagnostics",
            "action_id": action_id,
            "selection_files": context.selection_files,
        })
    }) {
        eprintln!("diagnostics runtime failed: {err:?}");
        std::process::exit(1);
    }
}
