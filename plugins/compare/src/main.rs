use plugin_api::{
    ActionEffects, ActionSpec, ConfirmPolicy, PluginHello, PluginRegister, RpcRequest, ViewSpec,
};

fn build_hello_request() -> RpcRequest {
    PluginHello {
        plugin_id: "compare".to_string(),
        version: "0.1".to_string(),
    }
    .to_request("hello-1")
}

fn build_register_request() -> RpcRequest {
    PluginRegister {
        actions: vec![ActionSpec {
            action_id: "compare.refs".to_string(),
            title: "Compare Refs".to_string(),
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
    .to_request("register-1")
}

fn main() {
    let hello = build_hello_request();
    let register = build_register_request();

    println!("{} -> {}", hello.method, register.method);
}
