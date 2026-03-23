use std::collections::HashMap;
use std::io::Read;
use std::process::{Child, ChildStderr, ChildStdin, ChildStdout, Command, Stdio};
use std::time::{Duration, Instant};

use plugin_api::{
    ActionContext, ActionSpec, CodecError, FrameCodec, HelloAck, METHOD_HOST_ACTION_INVOKE,
    METHOD_PLUGIN_READY, PluginHello, PluginRegister, RegisterAck, RpcMessage, RpcNotification,
    RpcRequest, RpcResponse,
};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PluginRegistration {
    pub plugin_id: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RestartPolicy {
    Never,
    Once,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ProcessHealth {
    Running,
    Restarted { exit_code: i32 },
    Exited { exit_code: i32 },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PluginProcessConfig {
    pub plugin_id: String,
    pub program: String,
    pub args: Vec<String>,
    pub restart_policy: RestartPolicy,
}

#[derive(Debug)]
pub enum ProcessError {
    Spawn(String),
    MissingStdin,
    MissingStdout,
    MissingStderr,
    Io(std::io::Error),
    Codec(CodecError),
    RestartExhausted,
}

impl From<std::io::Error> for ProcessError {
    fn from(value: std::io::Error) -> Self {
        Self::Io(value)
    }
}

impl From<CodecError> for ProcessError {
    fn from(value: CodecError) -> Self {
        Self::Codec(value)
    }
}

#[derive(Debug)]
pub struct PluginProcess {
    config: PluginProcessConfig,
    child: Child,
    stdin: ChildStdin,
    stdout: ChildStdout,
    stderr: ChildStderr,
    codec: FrameCodec,
    restart_used: bool,
}

impl PluginProcess {
    pub fn spawn(config: PluginProcessConfig) -> Result<Self, ProcessError> {
        let (child, stdin, stdout, stderr) = spawn_child(&config)?;
        Ok(Self {
            config,
            child,
            stdin,
            stdout,
            stderr,
            codec: FrameCodec::default(),
            restart_used: false,
        })
    }

    pub fn send(&mut self, message: &RpcMessage) -> Result<(), ProcessError> {
        self.codec.write_to(&mut self.stdin, message)?;
        Ok(())
    }

    pub fn plugin_id(&self) -> &str {
        &self.config.plugin_id
    }

    pub fn receive(&mut self) -> Result<RpcMessage, ProcessError> {
        self.codec
            .read_from(&mut self.stdout)
            .map_err(ProcessError::from)
    }

    pub fn check_health(&mut self) -> Result<ProcessHealth, ProcessError> {
        let exit = self.child.try_wait()?;
        let Some(status) = exit else {
            return Ok(ProcessHealth::Running);
        };

        let exit_code = status.code().unwrap_or(-1);
        if self.config.restart_policy == RestartPolicy::Never || self.restart_used {
            return Ok(ProcessHealth::Exited { exit_code });
        }

        let (child, stdin, stdout, stderr) = spawn_child(&self.config)?;
        self.child = child;
        self.stdin = stdin;
        self.stdout = stdout;
        self.stderr = stderr;
        self.restart_used = true;
        Ok(ProcessHealth::Restarted { exit_code })
    }

    pub fn restart_if_exited(&mut self) -> Result<bool, ProcessError> {
        let exit = self.child.try_wait()?;
        if exit.is_none() {
            return Ok(false);
        }

        if self.config.restart_policy == RestartPolicy::Never || self.restart_used {
            return Err(ProcessError::RestartExhausted);
        }

        let (child, stdin, stdout, stderr) = spawn_child(&self.config)?;
        self.child = child;
        self.stdin = stdin;
        self.stdout = stdout;
        self.stderr = stderr;
        self.restart_used = true;
        Ok(true)
    }

    pub fn shutdown(&mut self) -> Result<(), ProcessError> {
        if self.child.try_wait()?.is_none() {
            let kill_result = self.child.kill();
            if let Err(err) = kill_result {
                return Err(ProcessError::Io(err));
            }
        }

        let _wait = self.child.wait()?;
        Ok(())
    }

    pub fn read_stderr_to_string(&mut self) -> Result<String, ProcessError> {
        let mut buf = String::new();
        self.stderr.read_to_string(&mut buf)?;
        Ok(buf)
    }
}

fn spawn_child(
    config: &PluginProcessConfig,
) -> Result<(Child, ChildStdin, ChildStdout, ChildStderr), ProcessError> {
    let mut command = Command::new(&config.program);
    command
        .args(&config.args)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());

    let mut child = command
        .spawn()
        .map_err(|err| ProcessError::Spawn(err.to_string()))?;

    let stdin = match child.stdin.take() {
        Some(stdin) => stdin,
        None => return Err(ProcessError::MissingStdin),
    };
    let stdout = match child.stdout.take() {
        Some(stdout) => stdout,
        None => return Err(ProcessError::MissingStdout),
    };
    let stderr = match child.stderr.take() {
        Some(stderr) => stderr,
        None => return Err(ProcessError::MissingStderr),
    };

    Ok((child, stdin, stdout, stderr))
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RegistryError {
    DuplicateAction { action_id: String },
    DuplicateView { view_id: String },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SessionError {
    PluginIdMismatch { expected: String, actual: String },
    HelloRequired,
    Registry(RegistryError),
    UnknownAction { action_id: String },
    UnknownRequestId { request_id: String },
    UnexpectedMessage,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PluginAvailability {
    Ready,
    Unavailable { reason: String },
}

#[derive(Debug)]
pub struct PluginSupervisor {
    plugin_id: String,
    process: PluginProcess,
    availability: PluginAvailability,
    last_exit_code: Option<i32>,
}

impl PluginSupervisor {
    pub fn new(process: PluginProcess) -> Self {
        Self {
            plugin_id: process.plugin_id().to_string(),
            process,
            availability: PluginAvailability::Ready,
            last_exit_code: None,
        }
    }

    pub fn poll(&mut self) -> Result<(), ProcessError> {
        match self.process.check_health()? {
            ProcessHealth::Running => {}
            ProcessHealth::Restarted { exit_code } => {
                self.last_exit_code = Some(exit_code);
            }
            ProcessHealth::Exited { exit_code } => {
                self.last_exit_code = Some(exit_code);
                self.availability = PluginAvailability::Unavailable {
                    reason: format!("plugin exited with code {exit_code}"),
                };
            }
        }
        Ok(())
    }

    pub fn availability(&self) -> &PluginAvailability {
        &self.availability
    }

    pub fn last_exit_code(&self) -> Option<i32> {
        self.last_exit_code
    }

    pub fn plugin_id(&self) -> &str {
        &self.plugin_id
    }
}

impl From<RegistryError> for SessionError {
    fn from(value: RegistryError) -> Self {
        Self::Registry(value)
    }
}

#[derive(Debug, Default)]
pub struct PluginRegistry {
    action_owners: HashMap<String, String>,
    view_owners: HashMap<String, String>,
    actions: HashMap<String, ActionSpec>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RequestIdGenerator {
    next: u64,
}

impl Default for RequestIdGenerator {
    fn default() -> Self {
        Self { next: 1 }
    }
}

impl RequestIdGenerator {
    pub fn next_id(&mut self) -> String {
        let current = self.next;
        self.next += 1;
        format!("req-{current}")
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TimeoutPolicy {
    pub request_timeout: Duration,
}

impl Default for TimeoutPolicy {
    fn default() -> Self {
        Self {
            request_timeout: Duration::from_secs(5),
        }
    }
}

#[derive(Debug, Clone)]
pub struct PendingRequest {
    pub action_id: String,
    pub deadline: Instant,
}

#[derive(Debug, Default)]
pub struct PendingRequestMap {
    by_id: HashMap<String, PendingRequest>,
}

impl PendingRequestMap {
    pub fn insert(&mut self, request_id: String, pending: PendingRequest) {
        self.by_id.insert(request_id, pending);
    }

    pub fn resolve(&mut self, request_id: &str) -> Option<PendingRequest> {
        self.by_id.remove(request_id)
    }

    pub fn collect_timeouts(&mut self, now: Instant) -> Vec<String> {
        let mut expired = Vec::new();
        self.by_id.retain(|request_id, pending| {
            if pending.deadline <= now {
                expired.push(request_id.clone());
                false
            } else {
                true
            }
        });
        expired
    }

    pub fn len(&self) -> usize {
        self.by_id.len()
    }

    pub fn is_empty(&self) -> bool {
        self.by_id.is_empty()
    }
}

impl PluginRegistry {
    pub fn register(
        &mut self,
        plugin_id: &str,
        request: &PluginRegister,
    ) -> Result<RegisterAck, RegistryError> {
        for action in &request.actions {
            if self.action_owners.contains_key(&action.action_id) {
                return Err(RegistryError::DuplicateAction {
                    action_id: action.action_id.clone(),
                });
            }
        }

        for view in &request.views {
            if self.view_owners.contains_key(&view.view_id) {
                return Err(RegistryError::DuplicateView {
                    view_id: view.view_id.clone(),
                });
            }
        }

        for action in &request.actions {
            self.action_owners
                .insert(action.action_id.clone(), plugin_id.to_string());
            self.actions
                .insert(action.action_id.clone(), action.clone());
        }

        for view in &request.views {
            self.view_owners
                .insert(view.view_id.clone(), plugin_id.to_string());
        }

        Ok(RegisterAck {
            accepted_actions: request
                .actions
                .iter()
                .map(|a| a.action_id.clone())
                .collect(),
            accepted_views: request.views.iter().map(|v| v.view_id.clone()).collect(),
        })
    }

    pub fn action_owner(&self, action_id: &str) -> Option<&str> {
        self.action_owners.get(action_id).map(String::as_str)
    }

    pub fn actions(&self) -> Vec<ActionSpec> {
        self.actions.values().cloned().collect()
    }
}

#[derive(Debug)]
pub struct RuntimeSession {
    plugin_id: String,
    hello_done: bool,
    registry: PluginRegistry,
    request_ids: RequestIdGenerator,
    pending: PendingRequestMap,
    timeout_policy: TimeoutPolicy,
    subscriptions: Vec<String>,
    notification_outbox: Vec<RpcNotification>,
}

impl RuntimeSession {
    pub fn new(plugin_id: impl Into<String>) -> Self {
        Self {
            plugin_id: plugin_id.into(),
            hello_done: false,
            registry: PluginRegistry::default(),
            request_ids: RequestIdGenerator::default(),
            pending: PendingRequestMap::default(),
            timeout_policy: TimeoutPolicy::default(),
            subscriptions: Vec::new(),
            notification_outbox: Vec::new(),
        }
    }

    pub fn with_timeout_policy(
        plugin_id: impl Into<String>,
        timeout_policy: TimeoutPolicy,
    ) -> Self {
        Self {
            plugin_id: plugin_id.into(),
            hello_done: false,
            registry: PluginRegistry::default(),
            request_ids: RequestIdGenerator::default(),
            pending: PendingRequestMap::default(),
            timeout_policy,
            subscriptions: Vec::new(),
            notification_outbox: Vec::new(),
        }
    }

    pub fn handle_hello(&mut self, hello: &PluginHello) -> Result<HelloAck, SessionError> {
        if hello.plugin_id != self.plugin_id {
            return Err(SessionError::PluginIdMismatch {
                expected: self.plugin_id.clone(),
                actual: hello.plugin_id.clone(),
            });
        }

        self.hello_done = true;
        Ok(handle_hello(hello))
    }

    pub fn handle_register(
        &mut self,
        register: &PluginRegister,
    ) -> Result<RegisterAck, SessionError> {
        if !self.hello_done {
            return Err(SessionError::HelloRequired);
        }
        self.registry
            .register(&self.plugin_id, register)
            .map_err(SessionError::from)
    }

    pub fn ready_notification(&self) -> RpcMessage {
        RpcMessage::Notification(RpcNotification::new(
            METHOD_PLUGIN_READY,
            serde_json::json!({"plugin_id": self.plugin_id}),
        ))
    }

    pub fn invoke_action(
        &mut self,
        action_id: &str,
        context: ActionContext,
        now: Instant,
    ) -> Result<RpcRequest, SessionError> {
        if self.action_owner(action_id) != Some(self.plugin_id.as_str()) {
            return Err(SessionError::UnknownAction {
                action_id: action_id.to_string(),
            });
        }

        let request_id = self.request_ids.next_id();
        let params = serde_json::to_value(&context).unwrap_or_else(|_| serde_json::json!({}));
        let request = RpcRequest::new(
            request_id.clone(),
            METHOD_HOST_ACTION_INVOKE,
            serde_json::json!({
                "action_id": action_id,
                "context": params,
            }),
        );

        self.pending.insert(
            request_id,
            PendingRequest {
                action_id: action_id.to_string(),
                deadline: now + self.timeout_policy.request_timeout,
            },
        );

        Ok(request)
    }

    pub fn handle_inbound_message(
        &mut self,
        message: &RpcMessage,
    ) -> Result<Option<String>, SessionError> {
        match message {
            RpcMessage::Response(RpcResponse { id, .. }) => {
                let resolved = self.pending.resolve(id);
                match resolved {
                    Some(pending) => Ok(Some(pending.action_id)),
                    None => Err(SessionError::UnknownRequestId {
                        request_id: id.clone(),
                    }),
                }
            }
            RpcMessage::Notification(RpcNotification { method, .. }) => Ok(Some(method.clone())),
            RpcMessage::Request(_) => Err(SessionError::UnexpectedMessage),
        }
    }

    pub fn collect_timeouts(&mut self, now: Instant) -> Vec<String> {
        self.pending.collect_timeouts(now)
    }

    pub fn pending_count(&self) -> usize {
        self.pending.len()
    }

    pub fn action_owner(&self, action_id: &str) -> Option<&str> {
        self.registry.action_owner(action_id)
    }

    pub fn list_actions(&self) -> Vec<ActionSpec> {
        self.registry.actions()
    }

    pub fn subscribe(&mut self, method: &str) {
        if !self.subscriptions.iter().any(|s| s == method) {
            self.subscriptions.push(method.to_string());
        }
    }

    pub fn deliver_notification(&mut self, notification: RpcNotification) -> bool {
        if self.subscriptions.iter().any(|s| s == &notification.method) {
            self.notification_outbox.push(notification);
            true
        } else {
            false
        }
    }

    pub fn drain_notifications(&mut self) -> Vec<RpcNotification> {
        std::mem::take(&mut self.notification_outbox)
    }
}

pub fn handle_hello(_hello: &PluginHello) -> HelloAck {
    HelloAck {
        protocol_version: "0.1".to_string(),
        host_version: env!("CARGO_PKG_VERSION").to_string(),
    }
}

pub fn handshake(plugin_id: &str) -> (PluginRegistration, RpcMessage) {
    let registration = PluginRegistration {
        plugin_id: plugin_id.to_string(),
    };
    let ready = RpcMessage::Notification(RpcNotification::new(
        METHOD_PLUGIN_READY,
        serde_json::json!({"plugin_id": plugin_id}),
    ));
    (registration, ready)
}

pub fn default_registration_payload() -> PluginRegister {
    repo_manager_registration_payload()
}

pub fn repo_manager_registration_payload() -> PluginRegister {
    PluginRegister {
        actions: vec![ActionSpec {
            action_id: "repo.open".to_string(),
            title: "Open Repository".to_string(),
            when: Some("always".to_string()),
            params_schema: None,
        }],
        views: Vec::new(),
    }
}

pub fn status_registration_payload() -> PluginRegister {
    PluginRegister {
        actions: vec![
            ActionSpec {
                action_id: "index.stage_selected".to_string(),
                title: "Stage Selected".to_string(),
                when: Some("repo.is_open".to_string()),
                params_schema: None,
            },
            ActionSpec {
                action_id: "index.unstage_selected".to_string(),
                title: "Unstage Selected".to_string(),
                when: Some("repo.is_open".to_string()),
                params_schema: None,
            },
            ActionSpec {
                action_id: "commit.create".to_string(),
                title: "Commit".to_string(),
                when: Some("repo.is_open".to_string()),
                params_schema: None,
            },
            ActionSpec {
                action_id: "commit.amend".to_string(),
                title: "Amend Commit".to_string(),
                when: Some("repo.is_open".to_string()),
                params_schema: None,
            },
        ],
        views: vec![plugin_api::ViewSpec {
            view_id: "status.panel".to_string(),
            title: "Status".to_string(),
            slot: "left".to_string(),
            when: Some("repo.is_open".to_string()),
        }],
    }
}

pub fn history_registration_payload() -> PluginRegister {
    PluginRegister {
        actions: vec![
            ActionSpec {
                action_id: "history.load_more".to_string(),
                title: "Load More History".to_string(),
                when: Some("repo.is_open".to_string()),
                params_schema: None,
            },
            ActionSpec {
                action_id: "history.select_commit".to_string(),
                title: "Select Commit".to_string(),
                when: Some("repo.is_open".to_string()),
                params_schema: None,
            },
            ActionSpec {
                action_id: "history.search".to_string(),
                title: "Search History".to_string(),
                when: Some("repo.is_open".to_string()),
                params_schema: None,
            },
            ActionSpec {
                action_id: "history.clear_filter".to_string(),
                title: "Clear History Filter".to_string(),
                when: Some("repo.is_open".to_string()),
                params_schema: None,
            },
        ],
        views: vec![plugin_api::ViewSpec {
            view_id: "history.panel".to_string(),
            title: "History".to_string(),
            slot: "left".to_string(),
            when: Some("repo.is_open".to_string()),
        }],
    }
}

pub fn branches_registration_payload() -> PluginRegister {
    PluginRegister {
        actions: vec![
            ActionSpec {
                action_id: "branch.checkout".to_string(),
                title: "Checkout Branch".to_string(),
                when: Some("repo.is_open".to_string()),
                params_schema: None,
            },
            ActionSpec {
                action_id: "branch.create".to_string(),
                title: "Create Branch".to_string(),
                when: Some("repo.is_open".to_string()),
                params_schema: None,
            },
            ActionSpec {
                action_id: "branch.rename".to_string(),
                title: "Rename Branch".to_string(),
                when: Some("repo.is_open".to_string()),
                params_schema: None,
            },
            ActionSpec {
                action_id: "branch.delete".to_string(),
                title: "Delete Branch".to_string(),
                when: Some("repo.is_open".to_string()),
                params_schema: None,
            },
            ActionSpec {
                action_id: "tag.checkout".to_string(),
                title: "Checkout Tag".to_string(),
                when: Some("repo.is_open".to_string()),
                params_schema: None,
            },
            ActionSpec {
                action_id: "tag.create".to_string(),
                title: "Create Tag".to_string(),
                when: Some("repo.is_open".to_string()),
                params_schema: None,
            },
        ],
        views: vec![plugin_api::ViewSpec {
            view_id: "branches.panel".to_string(),
            title: "Branches".to_string(),
            slot: "left".to_string(),
            when: Some("repo.is_open".to_string()),
        }],
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use plugin_api::{
        METHOD_EVENT_REPO_OPENED, METHOD_EVENT_STATE_UPDATED, METHOD_PLUGIN_HELLO, RpcRequest,
        RpcResponse,
    };

    #[test]
    fn builds_ready_envelope() {
        let (_, ready) = handshake("repo_manager");
        assert!(matches!(
            ready,
            RpcMessage::Notification(RpcNotification { method, .. }) if method == METHOD_PLUGIN_READY
        ));
    }

    #[test]
    fn registry_rejects_duplicate_action() {
        let mut registry = PluginRegistry::default();
        let req1 = default_registration_payload();
        let req2 = default_registration_payload();

        let first = registry.register("repo_manager", &req1);
        assert!(first.is_ok());

        let second = registry.register("status", &req2);
        assert!(matches!(
            second,
            Err(RegistryError::DuplicateAction { action_id }) if action_id == "repo.open"
        ));
    }

    #[test]
    fn registry_rejects_duplicate_view() {
        let mut registry = PluginRegistry::default();
        let req1 = PluginRegister {
            actions: Vec::new(),
            views: vec![plugin_api::ViewSpec {
                view_id: "status.panel".to_string(),
                title: "Status".to_string(),
                slot: "left".to_string(),
                when: Some("always".to_string()),
            }],
        };
        let req2 = PluginRegister {
            actions: Vec::new(),
            views: vec![plugin_api::ViewSpec {
                view_id: "status.panel".to_string(),
                title: "Status".to_string(),
                slot: "left".to_string(),
                when: Some("always".to_string()),
            }],
        };

        let first = registry.register("repo_manager", &req1);
        assert!(first.is_ok());

        let second = registry.register("status", &req2);
        assert!(matches!(
            second,
            Err(RegistryError::DuplicateView { view_id }) if view_id == "status.panel"
        ));
    }

    #[test]
    fn hello_returns_protocol_ack() {
        let hello = PluginHello {
            plugin_id: "status".to_string(),
            version: "0.1".to_string(),
        };

        let ack = handle_hello(&hello);
        assert_eq!(ack.protocol_version, "0.1");
    }

    #[test]
    fn process_spawn_send_receive_and_shutdown() {
        let config = PluginProcessConfig {
            plugin_id: "echo".to_string(),
            program: "cat".to_string(),
            args: Vec::new(),
            restart_policy: RestartPolicy::Never,
        };
        let spawned = PluginProcess::spawn(config);
        assert!(spawned.is_ok());
        let mut process = match spawned {
            Ok(process) => process,
            Err(_) => return,
        };

        let outbound = RpcMessage::Request(RpcRequest::new(
            "r-1",
            METHOD_PLUGIN_HELLO,
            serde_json::json!({"plugin_id": "echo", "version": "0.1"}),
        ));

        let sent = process.send(&outbound);
        assert!(sent.is_ok());

        let received = process.receive();
        assert!(received.is_ok());
        if let Ok(actual) = received {
            assert_eq!(actual, outbound);
        }

        let shutdown = process.shutdown();
        assert!(shutdown.is_ok());
    }

    #[test]
    fn process_restart_once_when_exited() {
        let config = PluginProcessConfig {
            plugin_id: "oneshot".to_string(),
            program: "sh".to_string(),
            args: vec!["-c".to_string(), "exit 0".to_string()],
            restart_policy: RestartPolicy::Once,
        };

        let spawned = PluginProcess::spawn(config);
        assert!(spawned.is_ok());
        let mut process = match spawned {
            Ok(process) => process,
            Err(_) => return,
        };

        std::thread::sleep(std::time::Duration::from_millis(20));
        let restarted = process.restart_if_exited();
        assert!(matches!(restarted, Ok(true)));

        std::thread::sleep(std::time::Duration::from_millis(20));
        let exhausted = process.restart_if_exited();
        assert!(matches!(exhausted, Err(ProcessError::RestartExhausted)));
    }

    #[test]
    fn process_captures_stderr() {
        let config = PluginProcessConfig {
            plugin_id: "stderr".to_string(),
            program: "sh".to_string(),
            args: vec![
                "-c".to_string(),
                "echo runtime-error >&2; exit 0".to_string(),
            ],
            restart_policy: RestartPolicy::Never,
        };

        let spawned = PluginProcess::spawn(config);
        assert!(spawned.is_ok());
        let mut process = match spawned {
            Ok(process) => process,
            Err(_) => return,
        };

        std::thread::sleep(std::time::Duration::from_millis(20));
        let stderr_text = process.read_stderr_to_string();
        assert!(stderr_text.is_ok());
        if let Ok(text) = stderr_text {
            assert!(text.contains("runtime-error"));
        }
    }

    #[test]
    fn runtime_session_happy_path() {
        let mut session = RuntimeSession::new("status");
        let hello = PluginHello {
            plugin_id: "status".to_string(),
            version: "0.1".to_string(),
        };
        let ack = session.handle_hello(&hello);
        assert!(ack.is_ok());

        let register = default_registration_payload();
        let register_ack = session.handle_register(&register);
        assert!(register_ack.is_ok());
        assert_eq!(session.action_owner("repo.open"), Some("status"));

        let ready = session.ready_notification();
        assert!(matches!(
            ready,
            RpcMessage::Notification(RpcNotification { method, .. }) if method == METHOD_PLUGIN_READY
        ));

        let invoke = session.invoke_action(
            "repo.open",
            ActionContext {
                selection_files: vec!["README.md".to_string()],
            },
            Instant::now(),
        );
        assert!(invoke.is_ok());

        let req = match invoke {
            Ok(req) => req,
            Err(_) => return,
        };
        assert_eq!(req.method, METHOD_HOST_ACTION_INVOKE);
        assert_eq!(session.pending_count(), 1);

        let inbound = session.handle_inbound_message(&RpcMessage::Response(RpcResponse::ok(
            req.id,
            serde_json::json!({"ok": true}),
        )));
        assert!(inbound.is_ok());
        assert_eq!(session.pending_count(), 0);
    }

    #[test]
    fn runtime_session_rejects_register_before_hello() {
        let mut session = RuntimeSession::new("status");
        let register = default_registration_payload();
        let result = session.handle_register(&register);
        assert!(matches!(result, Err(SessionError::HelloRequired)));
    }

    #[test]
    fn runtime_session_rejects_unknown_response_id() {
        let mut session = RuntimeSession::new("status");
        let result = session.handle_inbound_message(&RpcMessage::Response(RpcResponse::ok(
            "unknown-id",
            serde_json::json!({"ok": true}),
        )));

        assert!(matches!(
            result,
            Err(SessionError::UnknownRequestId { request_id }) if request_id == "unknown-id"
        ));
    }

    #[test]
    fn runtime_session_collects_timeouts() {
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

        let register = default_registration_payload();
        let register_result = session.handle_register(&register);
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

        let expired = session.collect_timeouts(now + Duration::from_millis(2));
        assert_eq!(expired.len(), 1);
        assert_eq!(session.pending_count(), 0);
    }

    #[test]
    fn runtime_session_delivers_notifications() {
        let mut session = RuntimeSession::new("status");
        let incoming = RpcMessage::Notification(RpcNotification::new(
            METHOD_EVENT_STATE_UPDATED,
            serde_json::json!({"reason": "refresh"}),
        ));

        let result = session.handle_inbound_message(&incoming);
        assert!(result.is_ok());
        if let Ok(Some(method)) = result {
            assert_eq!(method, METHOD_EVENT_STATE_UPDATED);
        }
    }

    #[test]
    fn runtime_session_lists_registered_actions() {
        let mut session = RuntimeSession::new("status");
        let hello = PluginHello {
            plugin_id: "status".to_string(),
            version: "0.1".to_string(),
        };
        assert!(session.handle_hello(&hello).is_ok());
        assert!(
            session
                .handle_register(&default_registration_payload())
                .is_ok()
        );

        let actions = session.list_actions();
        assert_eq!(actions.len(), 1);
        assert_eq!(actions[0].action_id, "repo.open");
    }

    #[test]
    fn runtime_session_delivers_only_subscribed_notifications() {
        let mut session = RuntimeSession::new("status");
        session.subscribe(METHOD_EVENT_STATE_UPDATED);

        let delivered = session.deliver_notification(RpcNotification::new(
            METHOD_EVENT_STATE_UPDATED,
            serde_json::json!({"reason": "refresh"}),
        ));
        assert!(delivered);

        let dropped = session.deliver_notification(RpcNotification::new(
            METHOD_EVENT_REPO_OPENED,
            serde_json::json!({"repo": "."}),
        ));
        assert!(!dropped);

        let outbox = session.drain_notifications();
        assert_eq!(outbox.len(), 1);
        assert_eq!(outbox[0].method, METHOD_EVENT_STATE_UPDATED);
    }

    #[test]
    fn runtime_session_drains_notifications_in_order() {
        let mut session = RuntimeSession::new("status");
        session.subscribe(METHOD_EVENT_STATE_UPDATED);
        session.subscribe(METHOD_EVENT_REPO_OPENED);

        assert!(session.deliver_notification(RpcNotification::new(
            METHOD_EVENT_STATE_UPDATED,
            serde_json::json!({"reason": "refresh"})
        )));
        assert!(session.deliver_notification(RpcNotification::new(
            METHOD_EVENT_REPO_OPENED,
            serde_json::json!({"repo": "."})
        )));

        let outbox = session.drain_notifications();
        assert_eq!(outbox.len(), 2);
        assert_eq!(outbox[0].method, METHOD_EVENT_STATE_UPDATED);
        assert_eq!(outbox[1].method, METHOD_EVENT_REPO_OPENED);
        assert!(session.drain_notifications().is_empty());
    }

    #[test]
    fn process_health_restarts_once_then_reports_exit() {
        let config = PluginProcessConfig {
            plugin_id: "status".to_string(),
            program: "sh".to_string(),
            args: vec!["-c".to_string(), "exit 0".to_string()],
            restart_policy: RestartPolicy::Once,
        };

        let mut process = PluginProcess::spawn(config).expect("spawn");
        std::thread::sleep(std::time::Duration::from_millis(20));

        let first = process.check_health().expect("health");
        assert!(matches!(first, ProcessHealth::Restarted { exit_code: 0 }));

        std::thread::sleep(std::time::Duration::from_millis(20));
        let second = process.check_health().expect("health");
        assert!(matches!(second, ProcessHealth::Exited { exit_code: 0 }));
    }

    #[test]
    fn supervisor_marks_unavailable_after_exhausted_restart() {
        let config = PluginProcessConfig {
            plugin_id: "status".to_string(),
            program: "sh".to_string(),
            args: vec!["-c".to_string(), "exit 1".to_string()],
            restart_policy: RestartPolicy::Never,
        };

        let process = PluginProcess::spawn(config).expect("spawn");
        let mut supervisor = PluginSupervisor::new(process);

        std::thread::sleep(std::time::Duration::from_millis(20));
        supervisor.poll().expect("poll");
        assert!(matches!(
            supervisor.availability(),
            PluginAvailability::Unavailable { .. }
        ));
        assert_eq!(supervisor.last_exit_code(), Some(1));
        assert_eq!(supervisor.plugin_id(), "status");
    }

    #[test]
    fn status_registration_payload_contains_view_and_actions() {
        let payload = status_registration_payload();
        assert!(
            payload
                .actions
                .iter()
                .any(|a| a.action_id == "index.stage_selected")
        );
        assert!(
            payload
                .actions
                .iter()
                .any(|a| a.action_id == "index.unstage_selected")
        );
        assert!(
            payload
                .actions
                .iter()
                .any(|a| a.action_id == "commit.create")
        );
        assert!(
            payload
                .actions
                .iter()
                .any(|a| a.action_id == "commit.amend")
        );
        assert!(payload.views.iter().any(|v| v.view_id == "status.panel"));
    }

    #[test]
    fn history_registration_payload_contains_view_and_actions() {
        let payload = history_registration_payload();
        assert!(
            payload
                .actions
                .iter()
                .any(|a| a.action_id == "history.load_more")
        );
        assert!(
            payload
                .actions
                .iter()
                .any(|a| a.action_id == "history.select_commit")
        );
        assert!(
            payload
                .actions
                .iter()
                .any(|a| a.action_id == "history.search")
        );
        assert!(
            payload
                .actions
                .iter()
                .any(|a| a.action_id == "history.clear_filter")
        );
        assert!(payload.views.iter().any(|v| v.view_id == "history.panel"));
    }

    #[test]
    fn branches_registration_payload_contains_view_and_actions() {
        let payload = branches_registration_payload();
        assert!(
            payload
                .actions
                .iter()
                .any(|a| a.action_id == "branch.checkout")
        );
        assert!(
            payload
                .actions
                .iter()
                .any(|a| a.action_id == "branch.create")
        );
        assert!(
            payload
                .actions
                .iter()
                .any(|a| a.action_id == "branch.rename")
        );
        assert!(
            payload
                .actions
                .iter()
                .any(|a| a.action_id == "branch.delete")
        );
        assert!(
            payload
                .actions
                .iter()
                .any(|a| a.action_id == "tag.checkout")
        );
        assert!(payload.actions.iter().any(|a| a.action_id == "tag.create"));
        assert!(payload.views.iter().any(|v| v.view_id == "branches.panel"));
    }
}
