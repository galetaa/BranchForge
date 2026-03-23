use plugin_api::{ActionSpec, METHOD_PLUGIN_READY, PluginHello, PluginRegister, ViewSpec};
use plugin_host::{RuntimeSession, SessionError};

fn register_payload(action_id: &str, view_id: &str) -> PluginRegister {
    PluginRegister {
        actions: vec![ActionSpec {
            action_id: action_id.to_string(),
            title: "Action".to_string(),
            when: Some("always".to_string()),
            params_schema: None,
            danger: None,
        }],
        views: vec![ViewSpec {
            view_id: view_id.to_string(),
            title: "View".to_string(),
            slot: "left".to_string(),
            when: Some("always".to_string()),
        }],
    }
}

#[test]
fn hello_register_ready_happy_path() {
    let mut session = RuntimeSession::new("status");
    let hello = PluginHello {
        plugin_id: "status".to_string(),
        version: "0.1".to_string(),
    };

    let hello_result = session.handle_hello(&hello);
    assert!(hello_result.is_ok());

    let register = register_payload("index.stage_selected", "status.panel");
    let register_result = session.handle_register(&register);
    assert!(register_result.is_ok());

    assert_eq!(session.action_owner("index.stage_selected"), Some("status"));

    let ready = session.ready_notification();
    assert!(matches!(
        ready,
        plugin_api::RpcMessage::Notification(plugin_api::RpcNotification { method, .. }) if method == METHOD_PLUGIN_READY
    ));
}

#[test]
fn mismatched_plugin_id_is_rejected() {
    let mut session = RuntimeSession::new("status");
    let hello = PluginHello {
        plugin_id: "repo_manager".to_string(),
        version: "0.1".to_string(),
    };

    let result = session.handle_hello(&hello);
    assert!(matches!(
        result,
        Err(SessionError::PluginIdMismatch { expected, actual }) if expected == "status" && actual == "repo_manager"
    ));
}
