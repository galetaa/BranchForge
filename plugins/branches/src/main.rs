use plugin_api::{ActionSpec, DangerLevel, PluginHello, PluginRegister, RpcRequest, ViewSpec};

fn build_hello_request() -> RpcRequest {
    PluginHello {
        plugin_id: "branches".to_string(),
        version: "0.1".to_string(),
    }
    .to_request("hello-1")
}

fn build_register_request() -> RpcRequest {
    PluginRegister {
        actions: vec![
            ActionSpec {
                action_id: "branch.checkout".to_string(),
                title: "Checkout Branch".to_string(),
                when: Some("repo.is_open".to_string()),
                params_schema: None,
                danger: None,
            },
            ActionSpec {
                action_id: "branch.create".to_string(),
                title: "Create Branch".to_string(),
                when: Some("repo.is_open".to_string()),
                params_schema: None,
                danger: None,
            },
            ActionSpec {
                action_id: "branch.rename".to_string(),
                title: "Rename Branch".to_string(),
                when: Some("repo.is_open".to_string()),
                params_schema: None,
                danger: None,
            },
            ActionSpec {
                action_id: "branch.delete".to_string(),
                title: "Delete Branch".to_string(),
                when: Some("repo.is_open".to_string()),
                params_schema: None,
                danger: Some(DangerLevel::High),
            },
            ActionSpec {
                action_id: "compare.refs".to_string(),
                title: "Compare Branches".to_string(),
                when: Some("repo.is_open".to_string()),
                params_schema: None,
                danger: None,
            },
            ActionSpec {
                action_id: "tag.checkout".to_string(),
                title: "Checkout Tag".to_string(),
                when: Some("repo.is_open".to_string()),
                params_schema: None,
                danger: Some(DangerLevel::Medium),
            },
            ActionSpec {
                action_id: "tag.create".to_string(),
                title: "Create Tag".to_string(),
                when: Some("repo.is_open".to_string()),
                params_schema: None,
                danger: Some(DangerLevel::Low),
            },
        ],
        views: vec![ViewSpec {
            view_id: "branches.panel".to_string(),
            title: "Branches".to_string(),
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
