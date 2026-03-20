use plugin_api::RpcEnvelope;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PluginRegistration {
    pub plugin_id: String,
}

pub fn handshake(plugin_id: &str) -> (PluginRegistration, RpcEnvelope) {
    let registration = PluginRegistration {
        plugin_id: plugin_id.to_string(),
    };
    let ready = RpcEnvelope::new(
        "ready-1",
        "plugin.ready",
        serde_json::json!({"plugin_id": plugin_id}),
    );
    (registration, ready)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn builds_ready_envelope() {
        let (_, ready) = handshake("repo_manager");
        assert_eq!(ready.method, "plugin.ready");
    }
}
