use plugin_api::{ActionSpec, PluginHello, PluginRegister, RpcRequest, ViewSpec};

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
            ActionSpec {
                action_id: "history.load_more".to_string(),
                title: "Load More History".to_string(),
                when: Some("repo.is_open".to_string()),
                params_schema: None,
            },
            ActionSpec {
                action_id: "history.select_commit".to_string(),
                title: "Select Commit".to_string(),
                when: Some("repo.is_open".to_string()),
                params_schema: None,
            },
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
