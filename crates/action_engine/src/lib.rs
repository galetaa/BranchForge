use std::time::Instant;

use plugin_api::{ActionContext, DangerLevel, METHOD_HOST_ACTION_INVOKE, RpcRequest};
use plugin_host::{RuntimeSession, SessionError};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ActionRequest {
    pub action: String,
    pub confirmed: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum InvokeError {
    InvalidAction,
    ConfirmationRequired {
        action_id: String,
        danger: DangerLevel,
    },
    Session(SessionError),
}

impl From<SessionError> for InvokeError {
    fn from(value: SessionError) -> Self {
        Self::Session(value)
    }
}

pub fn validate_action(request: &ActionRequest) -> bool {
    !request.action.trim().is_empty()
}

pub fn build_action_invoke(id: impl Into<String>, action: &str, ctx: ActionContext) -> RpcRequest {
    let params = serde_json::to_value(&ctx).unwrap_or_else(|_| serde_json::json!({}));
    RpcRequest::new(
        id,
        METHOD_HOST_ACTION_INVOKE,
        serde_json::json!({
            "action_id": action,
            "context": params,
        }),
    )
}

pub fn route_action_invoke(
    session: &mut RuntimeSession,
    action: &ActionRequest,
    ctx: ActionContext,
    now: Instant,
) -> Result<RpcRequest, InvokeError> {
    if !validate_action(action) {
        return Err(InvokeError::InvalidAction);
    }

    if !action.confirmed {
        session
            .list_actions()
            .into_iter()
            .find(|spec| spec.action_id == action.action)
            .ok_or_else(|| {
                InvokeError::Session(SessionError::UnknownAction {
                    action_id: action.action.clone(),
                })
            })?
            .danger
            .filter(|level| matches!(level, DangerLevel::High))
            .map(|level| InvokeError::ConfirmationRequired {
                action_id: action.action.clone(),
                danger: level.clone(),
            })
            .map_or(Ok(()), Err)?;
    }

    session
        .invoke_action(&action.action, ctx, now)
        .map_err(InvokeError::from)
}

pub fn route_action_response(
    session: &mut RuntimeSession,
    inbound: &plugin_api::RpcMessage,
) -> Result<Option<String>, InvokeError> {
    session
        .handle_inbound_message(inbound)
        .map_err(InvokeError::from)
}

#[cfg(test)]
mod tests {
    use super::*;
    use plugin_api::PluginHello;
    use plugin_host::{
        RuntimeSession, branches_registration_payload, default_registration_payload,
    };

    #[test]
    fn rejects_empty_action() {
        let req = ActionRequest {
            action: String::new(),
            confirmed: false,
        };
        assert!(!validate_action(&req));
    }

    #[test]
    fn builds_action_invoke_request() {
        let req = build_action_invoke(
            "req-1",
            "repo.open",
            ActionContext {
                selection_files: Vec::new(),
            },
        );
        assert_eq!(req.method, METHOD_HOST_ACTION_INVOKE);
        assert_eq!(req.id, "req-1");
    }

    #[test]
    fn route_action_invoke_happy_path() {
        let mut session = RuntimeSession::new("status");
        let hello = PluginHello {
            plugin_id: "status".to_string(),
            version: "0.1".to_string(),
        };
        let hello_result = session.handle_hello(&hello);
        assert!(hello_result.is_ok());

        let register_result = session.handle_register(&default_registration_payload());
        assert!(register_result.is_ok());

        let action = ActionRequest {
            action: "repo.open".to_string(),
            confirmed: false,
        };
        let routed = route_action_invoke(
            &mut session,
            &action,
            ActionContext {
                selection_files: Vec::new(),
            },
            Instant::now(),
        );
        assert!(routed.is_ok());
        assert_eq!(session.pending_count(), 1);
    }

    #[test]
    fn route_action_invoke_requires_confirmation_for_high_risk() {
        let mut session = RuntimeSession::new("branches");
        let hello = PluginHello {
            plugin_id: "branches".to_string(),
            version: "0.1".to_string(),
        };
        let hello_result = session.handle_hello(&hello);
        assert!(hello_result.is_ok());

        let register_result = session.handle_register(&branches_registration_payload());
        assert!(register_result.is_ok());

        let action = ActionRequest {
            action: "branch.delete".to_string(),
            confirmed: false,
        };
        let routed = route_action_invoke(
            &mut session,
            &action,
            ActionContext {
                selection_files: Vec::new(),
            },
            Instant::now(),
        );
        assert!(matches!(
            routed,
            Err(InvokeError::ConfirmationRequired { action_id, danger })
                if action_id == "branch.delete" && danger == DangerLevel::High
        ));
    }

    #[test]
    fn route_action_invoke_allows_confirmed_high_risk() {
        let mut session = RuntimeSession::new("branches");
        let hello = PluginHello {
            plugin_id: "branches".to_string(),
            version: "0.1".to_string(),
        };
        let hello_result = session.handle_hello(&hello);
        assert!(hello_result.is_ok());

        let register_result = session.handle_register(&branches_registration_payload());
        assert!(register_result.is_ok());

        let action = ActionRequest {
            action: "branch.delete".to_string(),
            confirmed: true,
        };
        let routed = route_action_invoke(
            &mut session,
            &action,
            ActionContext {
                selection_files: Vec::new(),
            },
            Instant::now(),
        );
        assert!(routed.is_ok());
        assert_eq!(session.pending_count(), 1);
    }

    #[test]
    fn route_action_invoke_rejects_invalid_action() {
        let mut session = RuntimeSession::new("status");
        let action = ActionRequest {
            action: "  ".to_string(),
            confirmed: false,
        };
        let routed = route_action_invoke(
            &mut session,
            &action,
            ActionContext {
                selection_files: Vec::new(),
            },
            Instant::now(),
        );

        assert!(matches!(routed, Err(InvokeError::InvalidAction)));
    }

    #[test]
    fn route_action_invoke_propagates_unknown_action() {
        let mut session = RuntimeSession::new("status");
        let hello = PluginHello {
            plugin_id: "status".to_string(),
            version: "0.1".to_string(),
        };
        let hello_result = session.handle_hello(&hello);
        assert!(hello_result.is_ok());

        let register_result = session.handle_register(&default_registration_payload());
        assert!(register_result.is_ok());

        let action = ActionRequest {
            action: "unknown.action".to_string(),
            confirmed: false,
        };
        let routed = route_action_invoke(
            &mut session,
            &action,
            ActionContext {
                selection_files: Vec::new(),
            },
            Instant::now(),
        );

        assert!(matches!(
            routed,
            Err(InvokeError::Session(SessionError::UnknownAction { action_id })) if action_id == "unknown.action"
        ));
    }

    #[test]
    fn route_action_response_resolves_pending_request() {
        let mut session = RuntimeSession::new("status");
        let hello = PluginHello {
            plugin_id: "status".to_string(),
            version: "0.1".to_string(),
        };
        let hello_result = session.handle_hello(&hello);
        assert!(hello_result.is_ok());

        let register_result = session.handle_register(&default_registration_payload());
        assert!(register_result.is_ok());

        let action = ActionRequest {
            action: "repo.open".to_string(),
            confirmed: false,
        };
        let request_result = route_action_invoke(
            &mut session,
            &action,
            ActionContext {
                selection_files: Vec::new(),
            },
            Instant::now(),
        );
        assert!(request_result.is_ok());

        let request = match request_result {
            Ok(request) => request,
            Err(_) => return,
        };

        let inbound = plugin_api::RpcMessage::Response(plugin_api::RpcResponse::ok(
            request.id,
            serde_json::json!({"ok": true}),
        ));

        let resolved = route_action_response(&mut session, &inbound);
        assert!(resolved.is_ok());
        if let Ok(Some(action_id)) = resolved {
            assert_eq!(action_id, "repo.open");
        }
    }
}
