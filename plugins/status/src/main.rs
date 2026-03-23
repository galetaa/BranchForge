use plugin_api::{ActionSpec, DangerLevel, PluginHello, PluginRegister, RpcRequest, ViewSpec};

fn build_hello_request() -> RpcRequest {
    PluginHello {
        plugin_id: "status".to_string(),
        version: "0.1".to_string(),
    }
    .to_request("hello-1")
}

fn build_register_request() -> RpcRequest {
    PluginRegister {
        actions: vec![
            ActionSpec {
                action_id: "index.stage_selected".to_string(),
                title: "Stage Selected".to_string(),
                when: Some("repo.is_open".to_string()),
                params_schema: None,
                danger: None,
            },
            ActionSpec {
                action_id: "index.unstage_selected".to_string(),
                title: "Unstage Selected".to_string(),
                when: Some("repo.is_open".to_string()),
                params_schema: None,
                danger: None,
            },
            ActionSpec {
                action_id: "index.stage_hunk".to_string(),
                title: "Stage Hunk".to_string(),
                when: Some("repo.is_open".to_string()),
                params_schema: None,
                danger: None,
            },
            ActionSpec {
                action_id: "index.unstage_hunk".to_string(),
                title: "Unstage Hunk".to_string(),
                when: Some("repo.is_open".to_string()),
                params_schema: None,
                danger: None,
            },
            ActionSpec {
                action_id: "commit.create".to_string(),
                title: "Commit".to_string(),
                when: Some("repo.is_open".to_string()),
                params_schema: None,
                danger: None,
            },
            ActionSpec {
                action_id: "commit.amend".to_string(),
                title: "Amend Commit".to_string(),
                when: Some("repo.is_open".to_string()),
                params_schema: None,
                danger: Some(DangerLevel::Medium),
            },
        ],
        views: vec![ViewSpec {
            view_id: "status.panel".to_string(),
            title: "Status".to_string(),
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
