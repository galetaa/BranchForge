use plugin_api::{
    ActionEffects, ActionSpec, ConfirmPolicy, DangerLevel, PluginHello, PluginRegister,
    RpcRequest, ViewSpec,
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
        danger: Some(DangerLevel::Medium),
        effects: ActionEffects {
            writes_refs: true,
            writes_index: true,
            writes_worktree: true,
            danger_level: DangerLevel::Medium,
            ..ActionEffects::default()
        },
        confirm_policy: ConfirmPolicy::OnDanger,
    }
}

fn build_hello_request() -> RpcRequest {
    PluginHello {
        plugin_id: "history".to_string(),
        version: "0.1".to_string(),
    }
    .to_request("hello-1")
}

fn build_register_request() -> RpcRequest {
    PluginRegister {
        actions: vec![
            spec("history.load_more", "Load More History"),
            spec("history.select_commit", "Select Commit"),
            spec("history.search", "Search History"),
            spec("history.clear_filter", "Clear History Filter"),
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
    .to_request("register-1")
}

fn main() {
    let hello = build_hello_request();
    let register = build_register_request();

    println!("{} -> {}", hello.method, register.method);
}
