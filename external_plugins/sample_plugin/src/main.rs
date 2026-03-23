use plugin_sdk::{ActionSpec, PluginHello, PluginRegister, RpcRequest, ViewSpec};

fn build_hello_request() -> RpcRequest {
    PluginHello {
        plugin_id: "sample_external".to_string(),
        version: "0.1.0".to_string(),
    }
    .to_request("hello-1")
}

fn build_register_request() -> RpcRequest {
    PluginRegister {
        actions: vec![ActionSpec {
            action_id: "sample.ping".to_string(),
            title: "Sample Ping".to_string(),
            when: Some("always".to_string()),
            params_schema: None,
            danger: None,
        }],
        views: vec![ViewSpec {
            view_id: "sample.panel".to_string(),
            title: "Sample Panel".to_string(),
            slot: "left".to_string(),
            when: Some("always".to_string()),
        }],
    }
    .to_request("register-1")
}

fn main() {
    let hello = build_hello_request();
    let register = build_register_request();

    println!("{} -> {}", hello.method, register.method);
}
