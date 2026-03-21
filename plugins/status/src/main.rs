use plugin_api::RpcRequest;

fn main() {
    let hello = RpcRequest::new(
        "hello-1",
        "plugin.hello",
        serde_json::json!({"plugin_id": "status", "version": "0.1"}),
    );

    println!("{}", hello.method);
}
