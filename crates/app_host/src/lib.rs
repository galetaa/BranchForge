use std::time::Instant;

use action_engine::{ActionRequest, InvokeError, route_action_invoke, route_action_response};
use plugin_api::{ActionContext, PluginHello, RpcMessage, RpcResponse};
use plugin_host::{RuntimeSession, default_registration_payload};

pub fn run_action_roundtrip(action_id: &str) -> Result<String, InvokeError> {
    let mut session = RuntimeSession::new("status");
    let hello = PluginHello {
        plugin_id: "status".to_string(),
        version: "0.1".to_string(),
    };

    let hello_result = session.handle_hello(&hello);
    if let Err(err) = hello_result {
        return Err(InvokeError::Session(err));
    }

    let register_result = session.handle_register(&default_registration_payload());
    if let Err(err) = register_result {
        return Err(InvokeError::Session(err));
    }

    let invoke = route_action_invoke(
        &mut session,
        &ActionRequest {
            action: action_id.to_string(),
        },
        ActionContext {
            selection_files: Vec::new(),
        },
        Instant::now(),
    )?;

    let inbound = RpcMessage::Response(RpcResponse::ok(invoke.id, serde_json::json!({"ok": true})));
    let resolved = route_action_response(&mut session, &inbound)?;
    Ok(resolved.unwrap_or_else(String::new))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn roundtrip_returns_action_id() {
        let result = run_action_roundtrip("repo.open");
        assert!(result.is_ok());
        if let Ok(action_id) = result {
            assert_eq!(action_id, "repo.open");
        }
    }
}
