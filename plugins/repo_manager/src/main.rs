use plugin_api::{
    ActionEffects, ActionSpec, ConfirmPolicy, DangerLevel, PluginHello, PluginRegister, RpcRequest,
};

fn build_hello_request() -> RpcRequest {
    PluginHello {
        plugin_id: "repo_manager".to_string(),
        version: "0.1".to_string(),
    }
    .to_request("hello-1")
}

fn build_register_request() -> RpcRequest {
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
                action_id: "submodule.list".to_string(),
                title: "List Submodules".to_string(),
                when: Some("repo.is_open".to_string()),
                params_schema: None,
                danger: None,
                effects: ActionEffects::read_only(),
                confirm_policy: ConfirmPolicy::Never,
            },
        ],
        views: Vec::new(),
    }
    .to_request("register-1")
}

fn main() {
    let hello = build_hello_request();
    let register = build_register_request();

    println!("{} -> {}", hello.method, register.method);
}
