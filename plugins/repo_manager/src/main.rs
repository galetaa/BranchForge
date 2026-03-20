use plugin_api::RpcEnvelope;

fn main() {
    let hello = RpcEnvelope::new(
        "hello-1",
        "plugin.hello",
        serde_json::json!({"plugin_id": "repo_manager"}),
    );

    println!("{}", hello.method);
}
