use plugin_api::{
    ActionEffects, ActionSpec, ConfirmPolicy, DangerLevel, PluginHello, PluginRegister, ViewSpec,
    serve_static_plugin,
};

fn spec(action_id: &str, title: &str) -> ActionSpec {
    ActionSpec {
        action_id: action_id.to_string(),
        title: title.to_string(),
        when: Some("repo.is_open".to_string()),
        params_schema: None,
        danger: None,
        effects: ActionEffects::read_only(),
        confirm_policy: ConfirmPolicy::Never,
    }
}

fn mutation_spec(action_id: &str, title: &str) -> ActionSpec {
    ActionSpec {
        action_id: action_id.to_string(),
        title: title.to_string(),
        when: Some("repo.is_open".to_string()),
        params_schema: None,
        danger: Some(DangerLevel::High),
        effects: ActionEffects {
            writes_refs: true,
            writes_index: true,
            writes_worktree: true,
            danger_level: DangerLevel::High,
            ..ActionEffects::default()
        },
        confirm_policy: ConfirmPolicy::Always,
    }
}

fn build_register_payload() -> PluginRegister {
    PluginRegister {
        actions: vec![
            spec("history.load_more", "Load More History"),
            spec("history.select_commit", "Select Commit"),
            spec("history.search", "Search History"),
            spec("history.clear_filter", "Clear History Filter"),
            spec("history.file", "File History"),
            spec("blame.file", "Blame File"),
            mutation_spec("cherry_pick.commit", "Cherry-pick Commit"),
            mutation_spec("revert.commit", "Revert Commit"),
        ],
        views: vec![ViewSpec {
            view_id: "history.panel".to_string(),
            title: "History".to_string(),
            slot: "left".to_string(),
            when: Some("repo.is_open".to_string()),
        }],
    }
}

fn main() {
    let hello = PluginHello {
        plugin_id: "history".to_string(),
        version: env!("CARGO_PKG_VERSION").to_string(),
    };
    let register = build_register_payload();

    if let Err(err) = serve_static_plugin(hello, register, |action_id, context| {
        serde_json::json!({
            "ok": true,
            "plugin_id": "history",
            "action_id": action_id,
            "selection_files": context.selection_files,
        })
    }) {
        eprintln!("history runtime failed: {err:?}");
        std::process::exit(1);
    }
}
