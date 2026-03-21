use std::time::{Duration, Instant};

use plugin_api::{
    ActionContext, METHOD_EVENT_STATE_UPDATED, PluginHello, RpcMessage, RpcNotification,
    RpcResponse,
};
use plugin_host::{RuntimeSession, SessionError, TimeoutPolicy, default_registration_payload};

#[test]
fn timeout_case_moves_request_out_of_pending() {
    let mut session = RuntimeSession::with_timeout_policy(
        "status",
        TimeoutPolicy {
            request_timeout: Duration::from_millis(1),
        },
    );

    let hello = PluginHello {
        plugin_id: "status".to_string(),
        version: "0.1".to_string(),
    };
    let hello_result = session.handle_hello(&hello);
    assert!(hello_result.is_ok());

    let register_result = session.handle_register(&default_registration_payload());
    assert!(register_result.is_ok());

    let now = Instant::now();
    let invoke_result = session.invoke_action(
        "repo.open",
        ActionContext {
            selection_files: Vec::new(),
        },
        now,
    );
    assert!(invoke_result.is_ok());
    assert_eq!(session.pending_count(), 1);

    let timed_out_ids = session.collect_timeouts(now + Duration::from_millis(5));
    assert_eq!(timed_out_ids.len(), 1);
    assert_eq!(session.pending_count(), 0);
}

#[test]
fn invalid_response_id_is_rejected() {
    let mut session = RuntimeSession::new("status");
    let result = session.handle_inbound_message(&RpcMessage::Response(RpcResponse::ok(
        "missing-id",
        serde_json::json!({"ok": true}),
    )));

    assert!(matches!(
        result,
        Err(SessionError::UnknownRequestId { request_id }) if request_id == "missing-id"
    ));
}

#[test]
fn notification_delivery_is_exposed_to_runtime() {
    let mut session = RuntimeSession::new("status");
    let msg = RpcMessage::Notification(RpcNotification::new(
        METHOD_EVENT_STATE_UPDATED,
        serde_json::json!({"reason": "refresh"}),
    ));

    let result = session.handle_inbound_message(&msg);
    assert!(result.is_ok());
    if let Ok(Some(method)) = result {
        assert_eq!(method, METHOD_EVENT_STATE_UPDATED);
    }
}
