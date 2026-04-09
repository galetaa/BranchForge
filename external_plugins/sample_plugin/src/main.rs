use plugin_sdk::{
    ActionEffects, ActionSpec, ConfirmPolicy, PluginHello, PluginRegister, ViewSpec,
    serve_static_plugin,
};

fn build_register_payload() -> PluginRegister {
    PluginRegister {
        actions: vec![ActionSpec {
            action_id: "sample.ping".to_string(),
            title: "Sample Ping".to_string(),
            when: Some("always".to_string()),
            params_schema: None,
            danger: None,
            effects: ActionEffects::read_only(),
            confirm_policy: ConfirmPolicy::Never,
        }],
        views: vec![ViewSpec {
            view_id: "sample.panel".to_string(),
            title: "Sample Panel".to_string(),
            slot: "left".to_string(),
            when: Some("always".to_string()),
        }],
    }
}

fn main() {
    let hello = PluginHello {
        plugin_id: "sample_external".to_string(),
        version: "0.1.0".to_string(),
    };
    let register = build_register_payload();

    if let Err(err) = serve_static_plugin(hello, register, |action_id, context| {
        serde_json::json!({
            "ok": true,
            "plugin_id": "sample_external",
            "action_id": action_id,
            "message": "sample_external handled request",
            "selection_files": context.selection_files,
        })
    }) {
        eprintln!("sample_external runtime failed: {err:?}");
        std::process::exit(1);
    }
}
