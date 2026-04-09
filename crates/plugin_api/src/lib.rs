use std::io::{Read, Write};

use serde::{Deserialize, Serialize};

pub const DEFAULT_MAX_FRAME_SIZE: usize = 4 * 1024 * 1024;
pub const METHOD_PLUGIN_HELLO: &str = "plugin.hello";
pub const METHOD_PLUGIN_REGISTER: &str = "plugin.register";
pub const METHOD_PLUGIN_READY: &str = "plugin.ready";
pub const METHOD_HOST_ACTION_INVOKE: &str = "host.action.invoke";
pub const METHOD_HOST_ACTION_PREFLIGHT: &str = "host.action.preflight";
pub const METHOD_HOST_ACTION_PREVIEW: &str = "host.action.preview";
pub const METHOD_EVENT_REPO_OPENED: &str = "event.repo.opened";
pub const METHOD_EVENT_STATE_UPDATED: &str = "event.state.updated";
pub const METHOD_EVENT_JOB_FINISHED: &str = "event.job.finished";
pub const HOST_PLUGIN_PROTOCOL_VERSION: &str = "0.1";
pub const PLUGIN_MANIFEST_VERSION_V1: &str = "1";

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum DangerLevel {
    #[default]
    Low,
    Medium,
    High,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct ActionEffects {
    #[serde(default)]
    pub writes_refs: bool,
    #[serde(default)]
    pub writes_index: bool,
    #[serde(default)]
    pub writes_worktree: bool,
    #[serde(default)]
    pub network: bool,
    #[serde(default)]
    pub danger_level: DangerLevel,
}

impl ActionEffects {
    pub fn read_only() -> Self {
        Self::default()
    }

    pub fn mutating_worktree() -> Self {
        Self {
            writes_worktree: true,
            danger_level: DangerLevel::Medium,
            ..Self::default()
        }
    }

    pub fn mutating_refs() -> Self {
        Self {
            writes_refs: true,
            danger_level: DangerLevel::High,
            ..Self::default()
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum ConfirmPolicy {
    Never,
    #[default]
    OnDanger,
    Always,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ConflictState {
    Merge,
    Rebase,
    CherryPick,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RpcRequest {
    pub id: String,
    pub method: String,
    pub params: serde_json::Value,
}

impl RpcRequest {
    pub fn new(
        id: impl Into<String>,
        method: impl Into<String>,
        params: serde_json::Value,
    ) -> Self {
        Self {
            id: id.into(),
            method: method.into(),
            params,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RpcError {
    pub code: i32,
    pub message: String,
    pub data: Option<serde_json::Value>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RpcResponse {
    pub id: String,
    pub result: Option<serde_json::Value>,
    pub error: Option<RpcError>,
}

impl RpcResponse {
    pub fn ok(id: impl Into<String>, result: serde_json::Value) -> Self {
        Self {
            id: id.into(),
            result: Some(result),
            error: None,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RpcNotification {
    pub method: String,
    pub params: serde_json::Value,
}

impl RpcNotification {
    pub fn new(method: impl Into<String>, params: serde_json::Value) -> Self {
        Self {
            method: method.into(),
            params,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", content = "payload", rename_all = "snake_case")]
pub enum RpcMessage {
    Request(RpcRequest),
    Response(RpcResponse),
    Notification(RpcNotification),
}

#[derive(Debug)]
pub enum CodecError {
    Io(std::io::Error),
    FrameTooLarge { size: usize, max: usize },
    TruncatedFrame { expected: usize, actual: usize },
    InvalidJson(String),
    TrailingBytes { extra: usize },
}

impl From<std::io::Error> for CodecError {
    fn from(value: std::io::Error) -> Self {
        Self::Io(value)
    }
}

#[derive(Debug, Clone)]
pub struct FrameCodec {
    max_frame_size: usize,
}

impl Default for FrameCodec {
    fn default() -> Self {
        Self {
            max_frame_size: DEFAULT_MAX_FRAME_SIZE,
        }
    }
}

impl FrameCodec {
    pub fn new(max_frame_size: usize) -> Self {
        Self { max_frame_size }
    }

    pub fn encode(&self, message: &RpcMessage) -> Result<Vec<u8>, CodecError> {
        let payload =
            serde_json::to_vec(message).map_err(|e| CodecError::InvalidJson(e.to_string()))?;
        if payload.len() > self.max_frame_size {
            return Err(CodecError::FrameTooLarge {
                size: payload.len(),
                max: self.max_frame_size,
            });
        }

        let size = u32::try_from(payload.len()).map_err(|_| CodecError::FrameTooLarge {
            size: payload.len(),
            max: self.max_frame_size,
        })?;

        let mut out = Vec::with_capacity(4 + payload.len());
        out.extend_from_slice(&size.to_be_bytes());
        out.extend_from_slice(&payload);
        Ok(out)
    }

    pub fn decode(&self, framed: &[u8]) -> Result<RpcMessage, CodecError> {
        if framed.len() < 4 {
            return Err(CodecError::TruncatedFrame {
                expected: 4,
                actual: framed.len(),
            });
        }

        let size = u32::from_be_bytes([framed[0], framed[1], framed[2], framed[3]]) as usize;
        if size > self.max_frame_size {
            return Err(CodecError::FrameTooLarge {
                size,
                max: self.max_frame_size,
            });
        }

        let required = 4 + size;
        if framed.len() < required {
            return Err(CodecError::TruncatedFrame {
                expected: required,
                actual: framed.len(),
            });
        }

        if framed.len() > required {
            return Err(CodecError::TrailingBytes {
                extra: framed.len() - required,
            });
        }

        serde_json::from_slice::<RpcMessage>(&framed[4..required])
            .map_err(|e| CodecError::InvalidJson(e.to_string()))
    }

    pub fn write_to<W: Write>(
        &self,
        writer: &mut W,
        message: &RpcMessage,
    ) -> Result<(), CodecError> {
        let frame = self.encode(message)?;
        writer.write_all(&frame)?;
        Ok(())
    }

    pub fn read_from<R: Read>(&self, reader: &mut R) -> Result<RpcMessage, CodecError> {
        let mut header = [0_u8; 4];
        reader.read_exact(&mut header)?;

        let size = u32::from_be_bytes(header) as usize;
        if size > self.max_frame_size {
            return Err(CodecError::FrameTooLarge {
                size,
                max: self.max_frame_size,
            });
        }

        let mut payload = vec![0_u8; size];
        reader.read_exact(&mut payload)?;

        serde_json::from_slice::<RpcMessage>(&payload)
            .map_err(|e| CodecError::InvalidJson(e.to_string()))
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RepoSnapshot {
    pub root: String,
    pub head: Option<String>,
    pub conflict_state: Option<ConflictState>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PluginManifestV1 {
    pub manifest_version: String,
    pub plugin_id: String,
    pub version: String,
    pub protocol_version: String,
    pub entrypoint: String,
    #[serde(default)]
    pub description: Option<String>,
    #[serde(default)]
    pub permissions: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PluginHello {
    pub plugin_id: String,
    pub version: String,
}

impl PluginHello {
    pub fn to_request(&self, id: impl Into<String>) -> RpcRequest {
        let params = serde_json::to_value(self).unwrap_or_else(|_| serde_json::json!({}));
        RpcRequest::new(id, METHOD_PLUGIN_HELLO, params)
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct HelloAck {
    pub protocol_version: String,
    pub host_version: String,
}

impl HelloAck {
    pub fn to_response(&self, id: impl Into<String>) -> RpcResponse {
        let result = serde_json::to_value(self).unwrap_or_else(|_| serde_json::json!({}));
        RpcResponse::ok(id, result)
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ActionSpec {
    pub action_id: String,
    pub title: String,
    pub when: Option<String>,
    pub params_schema: Option<serde_json::Value>,
    pub danger: Option<DangerLevel>,
    #[serde(default)]
    pub effects: ActionEffects,
    #[serde(default)]
    pub confirm_policy: ConfirmPolicy,
}

impl ActionSpec {
    pub fn effective_danger(&self) -> DangerLevel {
        self.danger
            .clone()
            .unwrap_or_else(|| self.effects.danger_level.clone())
    }

    pub fn requires_confirmation(&self) -> bool {
        match self.confirm_policy {
            ConfirmPolicy::Never => false,
            ConfirmPolicy::Always => true,
            ConfirmPolicy::OnDanger => matches!(self.effective_danger(), DangerLevel::High),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ViewSpec {
    pub view_id: String,
    pub title: String,
    pub slot: String,
    pub when: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PluginRegister {
    pub actions: Vec<ActionSpec>,
    pub views: Vec<ViewSpec>,
}

impl PluginRegister {
    pub fn to_request(&self, id: impl Into<String>) -> RpcRequest {
        let params = serde_json::to_value(self).unwrap_or_else(|_| serde_json::json!({}));
        RpcRequest::new(id, METHOD_PLUGIN_REGISTER, params)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RegisterAck {
    pub accepted_actions: Vec<String>,
    pub accepted_views: Vec<String>,
}

impl RegisterAck {
    pub fn to_response(&self, id: impl Into<String>) -> RpcResponse {
        let result = serde_json::to_value(self).unwrap_or_else(|_| serde_json::json!({}));
        RpcResponse::ok(id, result)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ActionContext {
    pub selection_files: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ActionPreflightRequest {
    pub action_id: String,
    pub context: ActionContext,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ActionPreflightResult {
    pub action_id: String,
    pub ok: bool,
    pub warnings: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ActionPreview {
    pub action_id: String,
    pub title: String,
    pub summary: String,
    pub warnings: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ConfirmRequest {
    pub action_id: String,
    pub danger: DangerLevel,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RepoOpenedEvent {
    pub repo: RepoSnapshot,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct StateUpdatedEvent {
    pub reason: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct JobFinishedEvent {
    pub job_id: String,
    pub success: bool,
}

#[derive(Debug)]
pub enum StaticPluginRuntimeError {
    Io(std::io::Error),
    Codec(CodecError),
    InvalidHostMessage { message: String },
    InvalidPayload { method: String, reason: String },
}

impl From<std::io::Error> for StaticPluginRuntimeError {
    fn from(value: std::io::Error) -> Self {
        Self::Io(value)
    }
}

impl From<CodecError> for StaticPluginRuntimeError {
    fn from(value: CodecError) -> Self {
        Self::Codec(value)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
struct HostActionInvokeRequest {
    action_id: String,
    context: ActionContext,
}

pub fn serve_static_plugin<F>(
    hello: PluginHello,
    register: PluginRegister,
    mut handle_action: F,
) -> Result<(), StaticPluginRuntimeError>
where
    F: FnMut(&str, &ActionContext) -> serde_json::Value,
{
    let codec = FrameCodec::default();
    let stdin = std::io::stdin();
    let stdout = std::io::stdout();
    let mut reader = stdin.lock();
    let mut writer = stdout.lock();

    let hello_request = hello.to_request("hello-1");
    write_message(
        &codec,
        &mut writer,
        RpcMessage::Request(hello_request.clone()),
    )?;
    expect_ok_response(&codec, &mut reader, &hello_request.id, METHOD_PLUGIN_HELLO)?;

    let register_request = register.to_request("register-1");
    write_message(
        &codec,
        &mut writer,
        RpcMessage::Request(register_request.clone()),
    )?;
    expect_ok_response(
        &codec,
        &mut reader,
        &register_request.id,
        METHOD_PLUGIN_REGISTER,
    )?;

    loop {
        match codec.read_from(&mut reader) {
            Ok(RpcMessage::Request(request)) => {
                let response = respond_to_host_request(&register, request, &mut handle_action);
                write_message(&codec, &mut writer, RpcMessage::Response(response))?;
            }
            Ok(RpcMessage::Notification(_)) => {}
            Ok(RpcMessage::Response(_)) => {
                return Err(StaticPluginRuntimeError::InvalidHostMessage {
                    message: "plugin received an unexpected response frame from host".to_string(),
                });
            }
            Err(CodecError::Io(err)) if err.kind() == std::io::ErrorKind::UnexpectedEof => {
                return Ok(());
            }
            Err(err) => return Err(StaticPluginRuntimeError::from(err)),
        }
    }
}

fn write_message<W: Write>(
    codec: &FrameCodec,
    writer: &mut W,
    message: RpcMessage,
) -> Result<(), StaticPluginRuntimeError> {
    codec.write_to(writer, &message)?;
    writer.flush()?;
    Ok(())
}

fn expect_ok_response<R: Read>(
    codec: &FrameCodec,
    reader: &mut R,
    expected_id: &str,
    expected_method: &str,
) -> Result<(), StaticPluginRuntimeError> {
    let inbound = codec.read_from(reader)?;
    let response = match inbound {
        RpcMessage::Response(response) => response,
        RpcMessage::Request(request) => {
            return Err(StaticPluginRuntimeError::InvalidHostMessage {
                message: format!(
                    "expected response for {expected_method}, received request `{}`",
                    request.method
                ),
            });
        }
        RpcMessage::Notification(notification) => {
            return Err(StaticPluginRuntimeError::InvalidHostMessage {
                message: format!(
                    "expected response for {expected_method}, received notification `{}`",
                    notification.method
                ),
            });
        }
    };

    if response.id != expected_id {
        return Err(StaticPluginRuntimeError::InvalidHostMessage {
            message: format!(
                "expected response id `{expected_id}` for {expected_method}, received `{}`",
                response.id
            ),
        });
    }

    if let Some(error) = response.error {
        return Err(StaticPluginRuntimeError::InvalidHostMessage {
            message: format!(
                "host rejected {expected_method}: code={} message={}",
                error.code, error.message
            ),
        });
    }

    Ok(())
}

fn respond_to_host_request<F>(
    register: &PluginRegister,
    request: RpcRequest,
    handle_action: &mut F,
) -> RpcResponse
where
    F: FnMut(&str, &ActionContext) -> serde_json::Value,
{
    match request.method.as_str() {
        METHOD_HOST_ACTION_INVOKE => {
            match serde_json::from_value::<HostActionInvokeRequest>(request.params.clone()) {
                Ok(invoke) => RpcResponse::ok(
                    request.id,
                    handle_action(&invoke.action_id, &invoke.context),
                ),
                Err(err) => {
                    error_response(request.id, -32602, format!("invalid invoke payload: {err}"))
                }
            }
        }
        METHOD_HOST_ACTION_PREFLIGHT => {
            let action_id = request
                .params
                .get("action_id")
                .and_then(serde_json::Value::as_str)
                .unwrap_or_default()
                .to_string();
            let known = register
                .actions
                .iter()
                .any(|spec| spec.action_id == action_id);
            let warnings = if known || action_id.is_empty() {
                Vec::new()
            } else {
                vec![format!("unknown action `{action_id}`")]
            };
            RpcResponse::ok(
                request.id,
                serde_json::to_value(ActionPreflightResult {
                    action_id,
                    ok: known,
                    warnings,
                })
                .unwrap_or_else(|_| serde_json::json!({"ok": false})),
            )
        }
        METHOD_HOST_ACTION_PREVIEW => {
            let action_id = request
                .params
                .get("action_id")
                .and_then(serde_json::Value::as_str)
                .unwrap_or_default()
                .to_string();
            let title = register
                .actions
                .iter()
                .find(|spec| spec.action_id == action_id)
                .map(|spec| spec.title.clone())
                .unwrap_or_else(|| action_id.clone());
            RpcResponse::ok(
                request.id,
                serde_json::to_value(ActionPreview {
                    action_id: action_id.clone(),
                    title: title.clone(),
                    summary: format!("Ready to run {title}."),
                    warnings: Vec::new(),
                })
                .unwrap_or_else(|_| serde_json::json!({"action_id": action_id})),
            )
        }
        _ => error_response(
            request.id,
            -32601,
            format!("unsupported host method `{}`", request.method),
        ),
    }
}

fn error_response(id: String, code: i32, message: String) -> RpcResponse {
    RpcResponse {
        id,
        result: None,
        error: Some(RpcError {
            code,
            message,
            data: None,
        }),
    }
}

#[cfg(test)]
mod tests {
    use std::io::Cursor;

    use super::*;

    #[test]
    fn roundtrip_request_encode_decode() {
        let codec = FrameCodec::default();
        let message = RpcMessage::Request(RpcRequest::new(
            "1",
            "plugin.hello",
            serde_json::json!({"plugin_id": "status"}),
        ));

        let frame_result = codec.encode(&message);
        assert!(frame_result.is_ok());

        let frame = match frame_result {
            Ok(frame) => frame,
            Err(_) => return,
        };

        let decoded_result = codec.decode(&frame);
        assert!(decoded_result.is_ok());

        if let Ok(decoded) = decoded_result {
            assert_eq!(decoded, message);
        }
    }

    #[test]
    fn roundtrip_with_reader_writer() {
        let codec = FrameCodec::default();
        let message = RpcMessage::Notification(RpcNotification::new(
            "event.state.updated",
            serde_json::json!({"repo_open": true}),
        ));

        let mut io = Cursor::new(Vec::<u8>::new());
        let write_result = codec.write_to(&mut io, &message);
        assert!(write_result.is_ok());
        io.set_position(0);

        let decoded_result = codec.read_from(&mut io);
        assert!(decoded_result.is_ok());

        if let Ok(decoded) = decoded_result {
            assert_eq!(decoded, message);
        }
    }

    #[test]
    fn decode_rejects_truncated_frame() {
        let codec = FrameCodec::default();
        let message = RpcMessage::Request(RpcRequest::new(
            "1",
            "plugin.hello",
            serde_json::json!({"plugin_id": "status"}),
        ));
        let frame_result = codec.encode(&message);
        assert!(frame_result.is_ok());

        let mut frame = match frame_result {
            Ok(frame) => frame,
            Err(_) => return,
        };
        frame.pop();

        let decoded = codec.decode(&frame);
        assert!(matches!(decoded, Err(CodecError::TruncatedFrame { .. })));
    }

    #[test]
    fn decode_rejects_invalid_json_payload() {
        let codec = FrameCodec::default();
        let mut frame = Vec::new();
        frame.extend_from_slice(&3_u32.to_be_bytes());
        frame.extend_from_slice(b"bad");

        let decoded = codec.decode(&frame);
        assert!(matches!(decoded, Err(CodecError::InvalidJson(_))));
    }

    #[test]
    fn plugin_hello_builds_request() {
        let hello = PluginHello {
            plugin_id: "status".to_string(),
            version: "0.1".to_string(),
        };

        let request = hello.to_request("req-1");
        assert_eq!(request.method, METHOD_PLUGIN_HELLO);
        assert_eq!(request.id, "req-1");
    }

    #[test]
    fn register_roundtrip_json() {
        let register = PluginRegister {
            actions: vec![ActionSpec {
                action_id: "repo.open".to_string(),
                title: "Open Repository".to_string(),
                when: Some("always".to_string()),
                params_schema: None,
                danger: None,
                effects: ActionEffects::read_only(),
                confirm_policy: ConfirmPolicy::Never,
            }],
            views: vec![ViewSpec {
                view_id: "status.panel".to_string(),
                title: "Status".to_string(),
                slot: "left".to_string(),
                when: Some("repo.is_open".to_string()),
            }],
        };

        let as_json = serde_json::to_value(&register);
        assert!(as_json.is_ok());

        if let Ok(json) = as_json {
            let restored: Result<PluginRegister, _> = serde_json::from_value(json);
            assert!(restored.is_ok());
        }
    }

    #[test]
    fn event_payloads_roundtrip_json() {
        let repo_opened = RepoOpenedEvent {
            repo: RepoSnapshot {
                root: "/tmp/repo".to_string(),
                head: Some("main".to_string()),
                conflict_state: None,
            },
        };
        let state_updated = StateUpdatedEvent {
            reason: "status.refresh".to_string(),
        };
        let job_finished = JobFinishedEvent {
            job_id: "job-1".to_string(),
            success: true,
        };

        let repo_json = serde_json::to_value(&repo_opened);
        let state_json = serde_json::to_value(&state_updated);
        let job_json = serde_json::to_value(&job_finished);

        assert!(repo_json.is_ok());
        assert!(state_json.is_ok());
        assert!(job_json.is_ok());

        if let Ok(json) = repo_json {
            let restored: Result<RepoOpenedEvent, _> = serde_json::from_value(json);
            assert!(restored.is_ok());
        }
        if let Ok(json) = state_json {
            let restored: Result<StateUpdatedEvent, _> = serde_json::from_value(json);
            assert!(restored.is_ok());
        }
        if let Ok(json) = job_json {
            let restored: Result<JobFinishedEvent, _> = serde_json::from_value(json);
            assert!(restored.is_ok());
        }
    }

    #[test]
    fn preflight_and_preview_roundtrip_json() {
        let preflight = ActionPreflightRequest {
            action_id: "branch.delete".to_string(),
            context: ActionContext {
                selection_files: vec!["README.md".to_string()],
            },
        };
        let preview = ActionPreview {
            action_id: "branch.delete".to_string(),
            title: "Delete Branch".to_string(),
            summary: "Deletes the selected branch.".to_string(),
            warnings: vec!["Branch is not merged.".to_string()],
        };
        let result = ActionPreflightResult {
            action_id: "branch.delete".to_string(),
            ok: false,
            warnings: vec!["Branch is not merged.".to_string()],
        };

        let preflight_json = serde_json::to_value(&preflight);
        let preview_json = serde_json::to_value(&preview);
        let result_json = serde_json::to_value(&result);

        assert!(preflight_json.is_ok());
        assert!(preview_json.is_ok());
        assert!(result_json.is_ok());
    }

    #[test]
    fn action_spec_defaults_are_backward_compatible() {
        let raw = serde_json::json!({
            "action_id": "repo.open",
            "title": "Open Repository",
            "when": "always",
            "params_schema": null,
            "danger": null
        });

        let parsed: Result<ActionSpec, _> = serde_json::from_value(raw);
        assert!(parsed.is_ok());

        if let Ok(spec) = parsed {
            assert_eq!(spec.effects, ActionEffects::default());
            assert_eq!(spec.confirm_policy, ConfirmPolicy::OnDanger);
        }
    }
}
