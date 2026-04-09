use plugin_api::{
    ActionEffects, ActionSpec, ConfirmPolicy, DangerLevel, PluginHello, PluginRegister, ViewSpec,
    serve_static_plugin,
};

fn spec(
    action_id: &str,
    title: &str,
    danger: Option<DangerLevel>,
    confirm_policy: ConfirmPolicy,
) -> ActionSpec {
    ActionSpec {
        action_id: action_id.to_string(),
        title: title.to_string(),
        when: Some("repo.is_open".to_string()),
        params_schema: None,
        danger: danger.clone(),
        effects: ActionEffects {
            writes_refs: true,
            danger_level: danger.unwrap_or(DangerLevel::Medium),
            ..ActionEffects::default()
        },
        confirm_policy,
    }
}

fn build_register_payload() -> PluginRegister {
    PluginRegister {
        actions: vec![
            spec(
                "tag.create",
                "Create Tag",
                Some(DangerLevel::Low),
                ConfirmPolicy::Never,
            ),
            spec(
                "tag.delete",
                "Delete Tag",
                Some(DangerLevel::Medium),
                ConfirmPolicy::OnDanger,
            ),
            ActionSpec {
                action_id: "tag.checkout".to_string(),
                title: "Checkout Tag".to_string(),
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
        ],
        views: vec![ViewSpec {
            view_id: "tags.panel".to_string(),
            title: "Tags".to_string(),
            slot: "left".to_string(),
            when: Some("repo.is_open".to_string()),
        }],
    }
}

fn main() {
    let hello = PluginHello {
        plugin_id: "tags".to_string(),
        version: env!("CARGO_PKG_VERSION").to_string(),
    };
    let register = build_register_payload();

    if let Err(err) = serve_static_plugin(hello, register, |action_id, context| {
        serde_json::json!({
            "ok": true,
            "plugin_id": "tags",
            "action_id": action_id,
            "selection_files": context.selection_files,
        })
    }) {
        eprintln!("tags runtime failed: {err:?}");
        std::process::exit(1);
    }
}
