use plugin_api::{
    ActionEffects, ActionSpec, ConfirmPolicy, PluginHello, PluginRegister, ViewSpec,
    serve_static_plugin,
};

fn build_register_payload() -> PluginRegister {
    PluginRegister {
        actions: vec![ActionSpec {
            action_id: "compare.refs".to_string(),
            title: "Compare Branches".to_string(),
            when: Some("repo.is_open".to_string()),
            params_schema: None,
            danger: None,
            effects: ActionEffects::read_only(),
            confirm_policy: ConfirmPolicy::Never,
        }],
        views: vec![ViewSpec {
            view_id: "compare.panel".to_string(),
            title: "Compare".to_string(),
            slot: "left".to_string(),
            when: Some("repo.is_open".to_string()),
        }],
    }
}

fn main() {
    let hello = PluginHello {
        plugin_id: "compare".to_string(),
        version: env!("CARGO_PKG_VERSION").to_string(),
    };
    let register = build_register_payload();

    if let Err(err) = serve_static_plugin(hello, register, |action_id, context| {
        serde_json::json!({
            "ok": true,
            "plugin_id": "compare",
            "action_id": action_id,
            "selection_files": context.selection_files,
        })
    }) {
        eprintln!("compare runtime failed: {err:?}");
        std::process::exit(1);
    }
}
