use plugin_api::{
    ActionEffects, ActionSpec, ConfirmPolicy, DangerLevel, PluginHello, PluginRegister, RpcRequest,
    ViewSpec,
};

fn build_hello_request() -> RpcRequest {
    PluginHello {
        plugin_id: "tags".to_string(),
        version: env!("CARGO_PKG_VERSION").to_string(),
    }
    .to_request("hello-1")
}

fn spec(action_id: &str, title: &str, danger: Option<DangerLevel>) -> ActionSpec {
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
        confirm_policy: ConfirmPolicy::OnDanger,
    }
}

fn build_register_request() -> RpcRequest {
    PluginRegister {
        actions: vec![
            spec("tag.create", "Create Tag", Some(DangerLevel::Low)),
            spec("tag.delete", "Delete Tag", Some(DangerLevel::Medium)),
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
    .to_request("register-1")
}

fn main() {
    let hello = build_hello_request();
    let register = build_register_request();

    println!("{} -> {}", hello.method, register.method);
}
