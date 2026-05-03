use plugin_sdk::{
    ActionEffects, ActionSpec, ConfirmPolicy, PluginHello, PluginRegister, ViewSpec,
    serve_static_plugin,
};

fn register_payload() -> PluginRegister {
    PluginRegister {
        actions: vec![ActionSpec {
            action_id: "secure_template.inspect".to_string(),
            title: "Inspect Selection".to_string(),
            when: Some("repo.is_open".to_string()),
            params_schema: None,
            danger: None,
            effects: ActionEffects::read_only(),
            confirm_policy: ConfirmPolicy::Never,
        }],
        views: vec![ViewSpec {
            view_id: "secure_template.panel".to_string(),
            title: "Secure Template".to_string(),
            slot: "right".to_string(),
            when: Some("repo.is_open".to_string()),
        }],
    }
}

fn main() {
    let hello = PluginHello {
        plugin_id: "secure_template".to_string(),
        version: "0.1.0".to_string(),
    };

    if let Err(err) = serve_static_plugin(hello, register_payload(), |action_id, context| {
        serde_json::json!({
            "ok": true,
            "action_id": action_id,
            "selection_files": context.selection_files,
        })
    }) {
        eprintln!("secure_template runtime failed: {err:?}");
        std::process::exit(1);
    }
}
