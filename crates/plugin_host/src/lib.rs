use std::collections::HashMap;
use std::io::{Read, Write};
use std::net::TcpStream;
use std::path::{Path, PathBuf};
use std::process::{Child, ChildStderr, ChildStdin, ChildStdout, Command, Stdio};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use plugin_api::{
    ActionContext, ActionEffects, ActionSpec, CodecError, ConfirmPolicy, DangerLevel, FrameCodec,
    HOST_PLUGIN_PROTOCOL_VERSION, HelloAck, METHOD_HOST_ACTION_INVOKE, METHOD_PLUGIN_READY,
    PLUGIN_MANIFEST_VERSION_V1, PluginHello, PluginManifestV1, PluginRegister, RegisterAck,
    RpcMessage, RpcNotification, RpcRequest, RpcResponse,
};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InstalledPluginInfo {
    pub manifest: PluginManifestV1,
    pub enabled: bool,
    pub install_dir: PathBuf,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DiscoverablePluginInfo {
    pub manifest: PluginManifestV1,
    pub package_dir: PathBuf,
    pub manifest_url: Option<String>,
    pub entrypoint_url: Option<String>,
    pub summary: Option<String>,
    pub channel: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PluginManagerError {
    Io(String),
    InvalidManifest(String),
    InvalidRegistry(String),
    UnsupportedSource(String),
    IncompatiblePlugin {
        plugin_id: String,
        required_protocol: String,
        host_protocol: String,
    },
    AlreadyInstalled(String),
    NotInstalled(String),
    RegistryPluginNotFound(String),
}

impl From<std::io::Error> for PluginManagerError {
    fn from(value: std::io::Error) -> Self {
        Self::Io(value.to_string())
    }
}

pub fn install_local_plugin(
    plugin_package_dir: &Path,
    plugins_root: &Path,
) -> Result<InstalledPluginInfo, PluginManagerError> {
    let manifest = read_manifest(plugin_package_dir)?;
    validate_manifest_compatibility(&manifest)?;

    std::fs::create_dir_all(plugins_root)?;
    let install_dir = plugins_root.join(&manifest.plugin_id);
    if install_dir.exists() {
        return Err(PluginManagerError::AlreadyInstalled(manifest.plugin_id));
    }

    copy_dir_recursive(plugin_package_dir, &install_dir)?;
    write_enabled_state(&install_dir, true)?;

    Ok(InstalledPluginInfo {
        manifest,
        enabled: true,
        install_dir,
    })
}

pub fn set_plugin_enabled(
    plugins_root: &Path,
    plugin_id: &str,
    enabled: bool,
) -> Result<InstalledPluginInfo, PluginManagerError> {
    let install_dir = plugins_root.join(plugin_id);
    if !install_dir.exists() {
        return Err(PluginManagerError::NotInstalled(plugin_id.to_string()));
    }
    let manifest = read_manifest(&install_dir)?;
    write_enabled_state(&install_dir, enabled)?;
    Ok(InstalledPluginInfo {
        manifest,
        enabled,
        install_dir,
    })
}

pub fn remove_local_plugin(plugins_root: &Path, plugin_id: &str) -> Result<(), PluginManagerError> {
    let install_dir = plugins_root.join(plugin_id);
    if !install_dir.exists() {
        return Err(PluginManagerError::NotInstalled(plugin_id.to_string()));
    }
    std::fs::remove_dir_all(install_dir)?;
    Ok(())
}

pub fn list_installed_plugins(
    plugins_root: &Path,
) -> Result<Vec<InstalledPluginInfo>, PluginManagerError> {
    if !plugins_root.exists() {
        return Ok(Vec::new());
    }
    let mut plugins = Vec::new();
    for item in std::fs::read_dir(plugins_root)? {
        let entry = item?;
        let path = entry.path();
        if !path.is_dir() {
            continue;
        }
        let manifest = match read_manifest(&path) {
            Ok(manifest) => manifest,
            Err(_) => continue,
        };
        let enabled = read_enabled_state(&path).unwrap_or(true);
        plugins.push(InstalledPluginInfo {
            manifest,
            enabled,
            install_dir: path,
        });
    }
    plugins.sort_by(|a, b| a.manifest.plugin_id.cmp(&b.manifest.plugin_id));
    Ok(plugins)
}

pub fn discover_local_plugins(
    registry_path: &Path,
) -> Result<Vec<DiscoverablePluginInfo>, PluginManagerError> {
    let registry_source = resolve_registry_source_path(registry_path);
    let raw = fetch_source_text(&registry_source)?;
    let registry: PluginRegistryV1 = serde_json::from_str(&raw)
        .map_err(|e| PluginManagerError::InvalidRegistry(e.to_string()))?;
    if registry.registry_version != "1" {
        return Err(PluginManagerError::InvalidRegistry(
            "registry_version must be '1'".to_string(),
        ));
    }

    let mut plugins = Vec::new();
    for entry in registry.plugins {
        let (manifest, package_dir, manifest_url, entrypoint_url) =
            resolve_registry_entry(&registry_source, &entry)?;
        if manifest.plugin_id != entry.plugin_id {
            return Err(PluginManagerError::InvalidRegistry(format!(
                "registry plugin_id `{}` does not match manifest plugin_id `{}`",
                entry.plugin_id, manifest.plugin_id
            )));
        }
        validate_manifest_compatibility(&manifest)?;
        plugins.push(DiscoverablePluginInfo {
            manifest,
            package_dir,
            manifest_url,
            entrypoint_url,
            summary: entry.summary,
            channel: entry.channel,
        });
    }
    plugins.sort_by(|a, b| a.manifest.plugin_id.cmp(&b.manifest.plugin_id));
    Ok(plugins)
}

pub fn install_registry_plugin(
    registry_path: &Path,
    plugins_root: &Path,
    plugin_id: &str,
) -> Result<InstalledPluginInfo, PluginManagerError> {
    let discovered = discover_local_plugins(registry_path)?;
    let plugin = discovered
        .into_iter()
        .find(|plugin| plugin.manifest.plugin_id == plugin_id)
        .ok_or_else(|| PluginManagerError::RegistryPluginNotFound(plugin_id.to_string()))?;
    if let (Some(manifest_url), Some(entrypoint_url)) = (
        plugin.manifest_url.as_deref(),
        plugin.entrypoint_url.as_deref(),
    ) {
        install_remote_plugin(&plugin.manifest, manifest_url, entrypoint_url, plugins_root)
    } else {
        install_local_plugin(&plugin.package_dir, plugins_root)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Deserialize)]
struct PluginRegistryV1 {
    registry_version: String,
    plugins: Vec<PluginRegistryEntry>,
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Deserialize)]
struct PluginRegistryEntry {
    plugin_id: String,
    package_dir: Option<String>,
    manifest_url: Option<String>,
    entrypoint_url: Option<String>,
    summary: Option<String>,
    channel: Option<String>,
}

fn resolve_registry_file(registry_path: &Path) -> PathBuf {
    if registry_path.is_dir() {
        registry_path.join("registry.json")
    } else {
        registry_path.to_path_buf()
    }
}

fn resolve_registry_source_path(registry_path: &Path) -> String {
    let raw = registry_path.to_string_lossy();
    if is_source_url(raw.as_ref()) {
        raw.into_owned()
    } else {
        resolve_registry_file(registry_path).display().to_string()
    }
}

fn read_manifest(plugin_dir: &Path) -> Result<PluginManifestV1, PluginManagerError> {
    let manifest_path = plugin_dir.join("plugin.json");
    let raw = std::fs::read_to_string(&manifest_path)
        .map_err(|e| PluginManagerError::Io(format!("{}: {}", manifest_path.display(), e)))?;
    let manifest = parse_manifest(&raw, &manifest_path.display().to_string())?;
    let entrypoint = plugin_dir.join(&manifest.entrypoint);
    if !entrypoint.exists() {
        return Err(PluginManagerError::InvalidManifest(format!(
            "entrypoint does not exist: {}",
            entrypoint.display()
        )));
    }
    Ok(manifest)
}

fn parse_manifest(raw: &str, source: &str) -> Result<PluginManifestV1, PluginManagerError> {
    let manifest: PluginManifestV1 = serde_json::from_str(raw)
        .map_err(|e| PluginManagerError::InvalidManifest(format!("{source}: {}", e)))?;

    if manifest.manifest_version != PLUGIN_MANIFEST_VERSION_V1 {
        return Err(PluginManagerError::InvalidManifest(
            "manifest_version must be '1'".to_string(),
        ));
    }
    if manifest.plugin_id.trim().is_empty() {
        return Err(PluginManagerError::InvalidManifest(
            "plugin_id cannot be empty".to_string(),
        ));
    }
    if manifest.entrypoint.trim().is_empty() {
        return Err(PluginManagerError::InvalidManifest(
            "entrypoint cannot be empty".to_string(),
        ));
    }

    Ok(manifest)
}

fn resolve_registry_entry(
    registry_source: &str,
    entry: &PluginRegistryEntry,
) -> Result<(PluginManifestV1, PathBuf, Option<String>, Option<String>), PluginManagerError> {
    if let Some(package_dir) = entry.package_dir.as_deref() {
        let resolved = resolve_source(registry_source, package_dir)?;
        let package_path = source_to_local_path(&resolved)?;
        let manifest = read_manifest(&package_path)?;
        return Ok((manifest, package_path, None, None));
    }

    if let (Some(manifest_url), Some(entrypoint_url)) = (
        entry.manifest_url.as_deref(),
        entry.entrypoint_url.as_deref(),
    ) {
        let manifest_source = resolve_source(registry_source, manifest_url)?;
        let entrypoint_source = resolve_source(registry_source, entrypoint_url)?;
        let manifest = load_manifest_from_source(&manifest_source)?;
        return Ok((
            manifest,
            PathBuf::new(),
            Some(manifest_source),
            Some(entrypoint_source),
        ));
    }

    Err(PluginManagerError::InvalidRegistry(format!(
        "registry entry `{}` must define package_dir or manifest_url + entrypoint_url",
        entry.plugin_id
    )))
}

fn load_manifest_from_source(source: &str) -> Result<PluginManifestV1, PluginManagerError> {
    let raw = fetch_source_text(source)?;
    parse_manifest(&raw, source)
}

fn install_remote_plugin(
    manifest: &PluginManifestV1,
    manifest_source: &str,
    entrypoint_source: &str,
    plugins_root: &Path,
) -> Result<InstalledPluginInfo, PluginManagerError> {
    validate_manifest_compatibility(manifest)?;

    std::fs::create_dir_all(plugins_root)?;
    let install_dir = plugins_root.join(&manifest.plugin_id);
    if install_dir.exists() {
        return Err(PluginManagerError::AlreadyInstalled(
            manifest.plugin_id.clone(),
        ));
    }
    std::fs::create_dir_all(&install_dir)?;

    let install_result = (|| -> Result<(), PluginManagerError> {
        let manifest_raw = serde_json::to_string_pretty(manifest)
            .map_err(|e| PluginManagerError::InvalidManifest(e.to_string()))?;
        std::fs::write(install_dir.join("plugin.json"), manifest_raw)
            .map_err(|e| PluginManagerError::Io(format!("{}: {}", manifest_source, e)))?;

        let binary = fetch_source_bytes(entrypoint_source)?;
        let entrypoint_path = install_dir.join(&manifest.entrypoint);
        std::fs::write(&entrypoint_path, binary)
            .map_err(|e| PluginManagerError::Io(format!("{}: {}", entrypoint_path.display(), e)))?;
        set_executable_permissions(&entrypoint_path)?;
        write_enabled_state(&install_dir, true)?;
        Ok(())
    })();

    if let Err(error) = install_result {
        let _ = std::fs::remove_dir_all(&install_dir);
        return Err(error);
    }

    Ok(InstalledPluginInfo {
        manifest: manifest.clone(),
        enabled: true,
        install_dir,
    })
}

fn fetch_source_text(source: &str) -> Result<String, PluginManagerError> {
    let bytes = fetch_source_bytes(source)?;
    String::from_utf8(bytes)
        .map_err(|_| PluginManagerError::Io(format!("{source}: invalid utf-8 content")))
}

fn fetch_source_bytes(source: &str) -> Result<Vec<u8>, PluginManagerError> {
    if let Some(path) = source.strip_prefix("file://") {
        return std::fs::read(file_url_to_path(path)?)
            .map_err(|e| PluginManagerError::Io(format!("{source}: {e}")));
    }
    if source.starts_with("http://") {
        return http_get_bytes(source);
    }
    if source.starts_with("https://") {
        return Err(PluginManagerError::UnsupportedSource(
            "https registry sources are not supported in the embedded transport".to_string(),
        ));
    }
    std::fs::read(source).map_err(|e| PluginManagerError::Io(format!("{source}: {e}")))
}

fn source_to_local_path(source: &str) -> Result<PathBuf, PluginManagerError> {
    if let Some(path) = source.strip_prefix("file://") {
        return file_url_to_path(path);
    }
    if source.starts_with("http://") || source.starts_with("https://") {
        return Err(PluginManagerError::UnsupportedSource(format!(
            "package_dir must resolve to a local path, got `{source}`"
        )));
    }
    Ok(PathBuf::from(source))
}

fn resolve_source(base_source: &str, raw: &str) -> Result<String, PluginManagerError> {
    if is_source_url(raw) {
        return Ok(raw.to_string());
    }

    if base_source.starts_with("http://") {
        return join_http_source(base_source, raw);
    }

    let base_path = if let Some(path) = base_source.strip_prefix("file://") {
        file_url_to_path(path)?
    } else {
        PathBuf::from(base_source)
    };

    let resolved = if Path::new(raw).is_absolute() {
        PathBuf::from(raw)
    } else {
        base_path
            .parent()
            .map(|parent| parent.join(raw))
            .unwrap_or_else(|| PathBuf::from(raw))
    };
    Ok(resolved.display().to_string())
}

fn is_source_url(raw: &str) -> bool {
    raw.starts_with("file://") || raw.starts_with("http://") || raw.starts_with("https://")
}

fn join_http_source(base_source: &str, raw: &str) -> Result<String, PluginManagerError> {
    let (authority, base_path) = split_http_source(base_source)?;
    let resolved_path = if raw.starts_with('/') {
        raw.to_string()
    } else {
        let base_dir = base_path
            .rsplit_once('/')
            .map(|(dir, _)| dir)
            .filter(|dir| !dir.is_empty())
            .unwrap_or("");
        if base_dir.is_empty() {
            format!("/{raw}")
        } else {
            format!("{base_dir}/{raw}")
        }
    };
    Ok(format!("http://{authority}{resolved_path}"))
}

fn split_http_source(source: &str) -> Result<(String, String), PluginManagerError> {
    let rest = source.strip_prefix("http://").ok_or_else(|| {
        PluginManagerError::UnsupportedSource(format!("unsupported registry source `{source}`"))
    })?;
    let (authority, path) = match rest.split_once('/') {
        Some((authority, path)) => (authority.to_string(), format!("/{}", path)),
        None => (rest.to_string(), "/".to_string()),
    };
    Ok((authority, path))
}

fn file_url_to_path(path: &str) -> Result<PathBuf, PluginManagerError> {
    if let Some(rest) = path.strip_prefix("localhost/") {
        return Ok(PathBuf::from(format!("/{}", rest)));
    }
    if path.starts_with('/') {
        return Ok(PathBuf::from(path));
    }
    Err(PluginManagerError::UnsupportedSource(format!(
        "unsupported file URL `file://{path}`"
    )))
}

fn http_get_bytes(source: &str) -> Result<Vec<u8>, PluginManagerError> {
    let (authority, path) = split_http_source(source)?;
    let (host, port) = match authority.rsplit_once(':') {
        Some((host, raw_port)) => {
            let port = raw_port.parse::<u16>().map_err(|_| {
                PluginManagerError::UnsupportedSource(format!(
                    "invalid port in registry source `{source}`"
                ))
            })?;
            (host.to_string(), port)
        }
        None => (authority.clone(), 80),
    };

    let mut stream = TcpStream::connect((host.as_str(), port))
        .map_err(|e| PluginManagerError::Io(format!("{source}: {e}")))?;
    let request = format!("GET {path} HTTP/1.1\r\nHost: {authority}\r\nConnection: close\r\n\r\n");
    stream
        .write_all(request.as_bytes())
        .map_err(|e| PluginManagerError::Io(format!("{source}: {e}")))?;
    let mut response = Vec::new();
    stream
        .read_to_end(&mut response)
        .map_err(|e| PluginManagerError::Io(format!("{source}: {e}")))?;

    let header_end = response
        .windows(4)
        .position(|window| window == b"\r\n\r\n")
        .ok_or_else(|| PluginManagerError::Io(format!("{source}: malformed HTTP response")))?;
    let header_text = String::from_utf8_lossy(&response[..header_end]);
    let status_line = header_text.lines().next().unwrap_or_default().to_string();
    if !status_line.contains(" 200 ") {
        return Err(PluginManagerError::Io(format!(
            "{source}: unexpected HTTP status `{status_line}`"
        )));
    }
    if header_text
        .lines()
        .any(|line| line.eq_ignore_ascii_case("transfer-encoding: chunked"))
    {
        return Err(PluginManagerError::UnsupportedSource(format!(
            "{source}: chunked HTTP responses are not supported"
        )));
    }

    Ok(response[(header_end + 4)..].to_vec())
}

fn set_executable_permissions(path: &Path) -> Result<(), PluginManagerError> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;

        let mut permissions = std::fs::metadata(path)
            .map_err(|e| PluginManagerError::Io(format!("{}: {}", path.display(), e)))?
            .permissions();
        permissions.set_mode(0o755);
        std::fs::set_permissions(path, permissions)
            .map_err(|e| PluginManagerError::Io(format!("{}: {}", path.display(), e)))?;
    }

    Ok(())
}

fn validate_manifest_compatibility(manifest: &PluginManifestV1) -> Result<(), PluginManagerError> {
    if manifest.protocol_version != HOST_PLUGIN_PROTOCOL_VERSION {
        return Err(PluginManagerError::IncompatiblePlugin {
            plugin_id: manifest.plugin_id.clone(),
            required_protocol: manifest.protocol_version.clone(),
            host_protocol: HOST_PLUGIN_PROTOCOL_VERSION.to_string(),
        });
    }
    Ok(())
}

fn write_enabled_state(install_dir: &Path, enabled: bool) -> Result<(), PluginManagerError> {
    let state = serde_json::json!({"enabled": enabled});
    std::fs::write(install_dir.join("state.json"), state.to_string())?;
    Ok(())
}

fn read_enabled_state(install_dir: &Path) -> Option<bool> {
    let raw = std::fs::read_to_string(install_dir.join("state.json")).ok()?;
    let value: serde_json::Value = serde_json::from_str(&raw).ok()?;
    value.get("enabled").and_then(|v| v.as_bool())
}

fn copy_dir_recursive(src: &Path, dst: &Path) -> Result<(), PluginManagerError> {
    std::fs::create_dir_all(dst)?;
    for item in std::fs::read_dir(src)? {
        let entry = item?;
        let path = entry.path();
        let target = dst.join(entry.file_name());
        if path.is_dir() {
            copy_dir_recursive(&path, &target)?;
        } else {
            std::fs::copy(&path, &target)?;
        }
    }
    Ok(())
}

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
    InvalidActionSpec { action_id: String, reason: String },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SessionError {
    PluginIdMismatch {
        expected: String,
        actual: String,
    },
    HelloRequired,
    InvalidLifecycleTransition {
        from: PluginLifecycleState,
        to: PluginLifecycleState,
    },
    PluginNotReady {
        plugin_id: String,
        state: PluginLifecycleState,
    },
    Registry(RegistryError),
    UnknownAction {
        action_id: String,
    },
    UnknownRequestId {
        request_id: String,
    },
    UnexpectedMessage,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PluginLifecycleState {
    Discovered,
    Starting,
    Handshaking,
    Registered,
    Ready,
    Degraded,
    Restarting,
    Stopped,
}

fn now_unix_ms() -> u128 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis())
        .unwrap_or(0)
}

pub fn format_runtime_log_event(
    plugin_id: &str,
    level: &str,
    event: &str,
    fields: serde_json::Value,
) -> String {
    serde_json::json!({
        "ts_ms": now_unix_ms(),
        "level": level,
        "source": "plugin_host",
        "plugin_id": plugin_id,
        "event": event,
        "fields": fields,
    })
    .to_string()
}

fn emit_runtime_log(plugin_id: &str, event: &str, fields: serde_json::Value) {
    eprintln!(
        "{}",
        format_runtime_log_event(plugin_id, "info", event, fields)
    );
}

fn emit_runtime_log_error(plugin_id: &str, event: &str, fields: serde_json::Value) {
    eprintln!(
        "{}",
        format_runtime_log_event(plugin_id, "error", event, fields)
    );
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

    pub fn poll(&mut self) -> Result<ProcessHealth, ProcessError> {
        let health = self.process.check_health()?;
        match health.clone() {
            ProcessHealth::Running => {
                self.availability = PluginAvailability::Ready;
                emit_runtime_log(
                    &self.plugin_id,
                    "runtime.supervisor.health",
                    serde_json::json!({"status": "running"}),
                );
            }
            ProcessHealth::Restarted { exit_code } => {
                self.last_exit_code = Some(exit_code);
                self.availability = PluginAvailability::Unavailable {
                    reason: format!("plugin restarting after exit code {exit_code}"),
                };
                emit_runtime_log(
                    &self.plugin_id,
                    "runtime.supervisor.health",
                    serde_json::json!({
                        "status": "restarted",
                        "exit_code": exit_code,
                    }),
                );
            }
            ProcessHealth::Exited { exit_code } => {
                self.last_exit_code = Some(exit_code);
                self.availability = PluginAvailability::Unavailable {
                    reason: format!("plugin exited with code {exit_code}"),
                };
                emit_runtime_log(
                    &self.plugin_id,
                    "runtime.supervisor.health",
                    serde_json::json!({
                        "status": "exited",
                        "exit_code": exit_code,
                    }),
                );
            }
        }
        Ok(health)
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
            validate_action_spec(action)?;
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

fn validate_action_spec(spec: &ActionSpec) -> Result<(), RegistryError> {
    if matches!(spec.confirm_policy, ConfirmPolicy::Never)
        && matches!(spec.effective_danger(), DangerLevel::High)
    {
        return Err(RegistryError::InvalidActionSpec {
            action_id: spec.action_id.clone(),
            reason: "high danger actions cannot disable confirmations".to_string(),
        });
    }
    Ok(())
}

#[derive(Debug)]
pub struct RuntimeSession {
    plugin_id: String,
    lifecycle_state: PluginLifecycleState,
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
            lifecycle_state: PluginLifecycleState::Discovered,
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
            lifecycle_state: PluginLifecycleState::Discovered,
            registry: PluginRegistry::default(),
            request_ids: RequestIdGenerator::default(),
            pending: PendingRequestMap::default(),
            timeout_policy,
            subscriptions: Vec::new(),
            notification_outbox: Vec::new(),
        }
    }

    pub fn lifecycle_state(&self) -> PluginLifecycleState {
        self.lifecycle_state
    }

    fn can_transition(from: PluginLifecycleState, to: PluginLifecycleState) -> bool {
        match from {
            PluginLifecycleState::Discovered => {
                matches!(
                    to,
                    PluginLifecycleState::Starting | PluginLifecycleState::Stopped
                )
            }
            PluginLifecycleState::Starting => matches!(
                to,
                PluginLifecycleState::Handshaking
                    | PluginLifecycleState::Degraded
                    | PluginLifecycleState::Stopped
            ),
            PluginLifecycleState::Handshaking => matches!(
                to,
                PluginLifecycleState::Registered
                    | PluginLifecycleState::Degraded
                    | PluginLifecycleState::Stopped
            ),
            PluginLifecycleState::Registered => matches!(
                to,
                PluginLifecycleState::Ready
                    | PluginLifecycleState::Degraded
                    | PluginLifecycleState::Stopped
            ),
            PluginLifecycleState::Ready => matches!(
                to,
                PluginLifecycleState::Degraded
                    | PluginLifecycleState::Restarting
                    | PluginLifecycleState::Stopped
            ),
            PluginLifecycleState::Degraded => {
                matches!(
                    to,
                    PluginLifecycleState::Restarting | PluginLifecycleState::Stopped
                )
            }
            PluginLifecycleState::Restarting => matches!(
                to,
                PluginLifecycleState::Starting
                    | PluginLifecycleState::Degraded
                    | PluginLifecycleState::Stopped
            ),
            PluginLifecycleState::Stopped => matches!(to, PluginLifecycleState::Starting),
        }
    }

    fn transition_to(&mut self, next: PluginLifecycleState) -> Result<(), SessionError> {
        let current = self.lifecycle_state;
        if Self::can_transition(current, next) {
            self.lifecycle_state = next;
            emit_runtime_log(
                &self.plugin_id,
                "runtime.lifecycle.transition",
                serde_json::json!({
                    "from": format!("{:?}", current),
                    "to": format!("{:?}", next),
                }),
            );
            return Ok(());
        }

        emit_runtime_log_error(
            &self.plugin_id,
            "runtime.lifecycle.transition_rejected",
            serde_json::json!({
                "from": format!("{:?}", current),
                "to": format!("{:?}", next),
            }),
        );
        Err(SessionError::InvalidLifecycleTransition {
            from: current,
            to: next,
        })
    }

    fn ensure_ready(&self) -> Result<(), SessionError> {
        if self.lifecycle_state == PluginLifecycleState::Ready {
            return Ok(());
        }
        Err(SessionError::PluginNotReady {
            plugin_id: self.plugin_id.clone(),
            state: self.lifecycle_state,
        })
    }

    pub fn handle_hello(&mut self, hello: &PluginHello) -> Result<HelloAck, SessionError> {
        self.transition_to(PluginLifecycleState::Starting)?;
        if hello.plugin_id != self.plugin_id {
            let _ = self.transition_to(PluginLifecycleState::Degraded);
            return Err(SessionError::PluginIdMismatch {
                expected: self.plugin_id.clone(),
                actual: hello.plugin_id.clone(),
            });
        }

        self.transition_to(PluginLifecycleState::Handshaking)?;
        Ok(handle_hello(hello))
    }

    pub fn handle_register(
        &mut self,
        register: &PluginRegister,
    ) -> Result<RegisterAck, SessionError> {
        if self.lifecycle_state != PluginLifecycleState::Handshaking {
            return Err(SessionError::HelloRequired);
        }

        let ack = self
            .registry
            .register(&self.plugin_id, register)
            .map_err(SessionError::from)?;
        self.transition_to(PluginLifecycleState::Registered)?;
        self.transition_to(PluginLifecycleState::Ready)?;
        Ok(ack)
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
        self.ensure_ready()?;
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
        if self.lifecycle_state != PluginLifecycleState::Ready {
            return None;
        }
        self.registry.action_owner(action_id)
    }

    pub fn list_actions(&self) -> Vec<ActionSpec> {
        if self.lifecycle_state != PluginLifecycleState::Ready {
            return Vec::new();
        }
        self.registry.actions()
    }

    pub fn mark_degraded(&mut self) -> Result<(), SessionError> {
        self.transition_to(PluginLifecycleState::Degraded)
    }

    pub fn mark_restarting(&mut self) -> Result<(), SessionError> {
        self.transition_to(PluginLifecycleState::Restarting)
    }

    pub fn mark_stopped(&mut self) -> Result<(), SessionError> {
        self.transition_to(PluginLifecycleState::Stopped)
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

fn spec(
    action_id: &str,
    title: &str,
    when: Option<&str>,
    danger: Option<DangerLevel>,
    effects: ActionEffects,
    confirm_policy: ConfirmPolicy,
) -> ActionSpec {
    ActionSpec {
        action_id: action_id.to_string(),
        title: title.to_string(),
        when: when.map(str::to_string),
        params_schema: None,
        danger,
        effects,
        confirm_policy,
    }
}

pub fn repo_manager_registration_payload() -> PluginRegister {
    PluginRegister {
        actions: vec![
            spec(
                "repo.open",
                "Open Repository",
                Some("always"),
                None,
                ActionEffects::read_only(),
                ConfirmPolicy::Never,
            ),
            spec(
                "worktree.list",
                "List Worktrees",
                Some("repo.is_open"),
                None,
                ActionEffects::read_only(),
                ConfirmPolicy::Never,
            ),
            spec(
                "worktree.create",
                "Create Worktree",
                Some("repo.is_open"),
                Some(DangerLevel::Medium),
                ActionEffects {
                    writes_refs: true,
                    writes_worktree: true,
                    danger_level: DangerLevel::Medium,
                    ..ActionEffects::default()
                },
                ConfirmPolicy::OnDanger,
            ),
            spec(
                "worktree.remove",
                "Remove Worktree",
                Some("repo.is_open"),
                Some(DangerLevel::High),
                ActionEffects {
                    writes_refs: true,
                    writes_worktree: true,
                    danger_level: DangerLevel::High,
                    ..ActionEffects::default()
                },
                ConfirmPolicy::Always,
            ),
            spec(
                "worktree.open",
                "Open Worktree",
                Some("repo.is_open"),
                None,
                ActionEffects::read_only(),
                ConfirmPolicy::Never,
            ),
            spec(
                "submodule.list",
                "List Submodules",
                Some("repo.is_open"),
                None,
                ActionEffects::read_only(),
                ConfirmPolicy::Never,
            ),
            spec(
                "submodule.init_update",
                "Init/Update Submodule",
                Some("repo.is_open"),
                Some(DangerLevel::Medium),
                ActionEffects {
                    writes_worktree: true,
                    danger_level: DangerLevel::Medium,
                    ..ActionEffects::default()
                },
                ConfirmPolicy::OnDanger,
            ),
            spec(
                "submodule.open",
                "Open Submodule",
                Some("repo.is_open"),
                None,
                ActionEffects::read_only(),
                ConfirmPolicy::Never,
            ),
        ],
        views: Vec::new(),
    }
}

pub fn status_registration_payload() -> PluginRegister {
    PluginRegister {
        actions: vec![
            spec(
                "index.stage_selected",
                "Stage Selected",
                Some("repo.is_open"),
                None,
                ActionEffects {
                    writes_index: true,
                    danger_level: DangerLevel::Low,
                    ..ActionEffects::default()
                },
                ConfirmPolicy::Never,
            ),
            spec(
                "index.unstage_selected",
                "Unstage Selected",
                Some("repo.is_open"),
                None,
                ActionEffects {
                    writes_index: true,
                    danger_level: DangerLevel::Low,
                    ..ActionEffects::default()
                },
                ConfirmPolicy::Never,
            ),
            spec(
                "index.stage_hunk",
                "Stage Hunk",
                Some("repo.is_open"),
                None,
                ActionEffects {
                    writes_index: true,
                    danger_level: DangerLevel::Low,
                    ..ActionEffects::default()
                },
                ConfirmPolicy::Never,
            ),
            spec(
                "index.stage_lines",
                "Stage Lines",
                Some("repo.is_open"),
                None,
                ActionEffects {
                    writes_index: true,
                    danger_level: DangerLevel::Low,
                    ..ActionEffects::default()
                },
                ConfirmPolicy::Never,
            ),
            spec(
                "index.unstage_hunk",
                "Unstage Hunk",
                Some("repo.is_open"),
                None,
                ActionEffects {
                    writes_index: true,
                    danger_level: DangerLevel::Low,
                    ..ActionEffects::default()
                },
                ConfirmPolicy::Never,
            ),
            spec(
                "index.unstage_lines",
                "Unstage Lines",
                Some("repo.is_open"),
                None,
                ActionEffects {
                    writes_index: true,
                    danger_level: DangerLevel::Low,
                    ..ActionEffects::default()
                },
                ConfirmPolicy::Never,
            ),
            spec(
                "commit.create",
                "Commit",
                Some("repo.is_open"),
                None,
                ActionEffects {
                    writes_refs: true,
                    writes_index: true,
                    danger_level: DangerLevel::Medium,
                    ..ActionEffects::default()
                },
                ConfirmPolicy::OnDanger,
            ),
            spec(
                "commit.amend",
                "Amend Commit",
                Some("repo.is_open"),
                Some(DangerLevel::Medium),
                ActionEffects {
                    writes_refs: true,
                    writes_index: true,
                    danger_level: DangerLevel::Medium,
                    ..ActionEffects::default()
                },
                ConfirmPolicy::OnDanger,
            ),
            spec(
                "file.discard",
                "Discard File Changes",
                Some("repo.is_open"),
                Some(DangerLevel::High),
                ActionEffects {
                    writes_worktree: true,
                    danger_level: DangerLevel::High,
                    ..ActionEffects::default()
                },
                ConfirmPolicy::Always,
            ),
            spec(
                "file.discard_hunk",
                "Discard Hunk",
                Some("repo.is_open"),
                Some(DangerLevel::High),
                ActionEffects {
                    writes_worktree: true,
                    danger_level: DangerLevel::High,
                    ..ActionEffects::default()
                },
                ConfirmPolicy::Always,
            ),
            spec(
                "file.discard_lines",
                "Discard Lines",
                Some("repo.is_open"),
                Some(DangerLevel::High),
                ActionEffects {
                    writes_worktree: true,
                    danger_level: DangerLevel::High,
                    ..ActionEffects::default()
                },
                ConfirmPolicy::Always,
            ),
            spec(
                "stash.create",
                "Create Stash",
                Some("repo.is_open"),
                Some(DangerLevel::Medium),
                ActionEffects {
                    writes_index: true,
                    writes_worktree: true,
                    danger_level: DangerLevel::Medium,
                    ..ActionEffects::default()
                },
                ConfirmPolicy::OnDanger,
            ),
            spec(
                "stash.list",
                "List Stashes",
                Some("repo.is_open"),
                None,
                ActionEffects::read_only(),
                ConfirmPolicy::Never,
            ),
            spec(
                "stash.apply",
                "Apply Stash",
                Some("repo.is_open"),
                Some(DangerLevel::Medium),
                ActionEffects {
                    writes_index: true,
                    writes_worktree: true,
                    danger_level: DangerLevel::Medium,
                    ..ActionEffects::default()
                },
                ConfirmPolicy::OnDanger,
            ),
            spec(
                "stash.pop",
                "Pop Stash",
                Some("repo.is_open"),
                Some(DangerLevel::Medium),
                ActionEffects {
                    writes_index: true,
                    writes_worktree: true,
                    danger_level: DangerLevel::Medium,
                    ..ActionEffects::default()
                },
                ConfirmPolicy::OnDanger,
            ),
            spec(
                "stash.drop",
                "Drop Stash",
                Some("repo.is_open"),
                Some(DangerLevel::Medium),
                ActionEffects {
                    writes_refs: true,
                    danger_level: DangerLevel::Medium,
                    ..ActionEffects::default()
                },
                ConfirmPolicy::OnDanger,
            ),
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
            spec(
                "history.load_more",
                "Load More History",
                Some("repo.is_open"),
                None,
                ActionEffects::read_only(),
                ConfirmPolicy::Never,
            ),
            spec(
                "history.select_commit",
                "Select Commit",
                Some("repo.is_open"),
                None,
                ActionEffects::read_only(),
                ConfirmPolicy::Never,
            ),
            spec(
                "history.search",
                "Search History",
                Some("repo.is_open"),
                None,
                ActionEffects::read_only(),
                ConfirmPolicy::Never,
            ),
            spec(
                "history.clear_filter",
                "Clear History Filter",
                Some("repo.is_open"),
                None,
                ActionEffects::read_only(),
                ConfirmPolicy::Never,
            ),
            spec(
                "history.file",
                "File History",
                Some("repo.is_open"),
                None,
                ActionEffects::read_only(),
                ConfirmPolicy::Never,
            ),
            spec(
                "blame.file",
                "Blame File",
                Some("repo.is_open"),
                None,
                ActionEffects::read_only(),
                ConfirmPolicy::Never,
            ),
            spec(
                "cherry_pick.commit",
                "Cherry-pick Commit",
                Some("repo.is_open"),
                Some(DangerLevel::Medium),
                ActionEffects {
                    writes_refs: true,
                    writes_index: true,
                    writes_worktree: true,
                    danger_level: DangerLevel::Medium,
                    ..ActionEffects::default()
                },
                ConfirmPolicy::OnDanger,
            ),
            spec(
                "revert.commit",
                "Revert Commit",
                Some("repo.is_open"),
                Some(DangerLevel::Medium),
                ActionEffects {
                    writes_refs: true,
                    writes_index: true,
                    writes_worktree: true,
                    danger_level: DangerLevel::Medium,
                    ..ActionEffects::default()
                },
                ConfirmPolicy::OnDanger,
            ),
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
            spec(
                "branch.checkout",
                "Checkout Branch",
                Some("repo.is_open"),
                None,
                ActionEffects {
                    writes_refs: true,
                    writes_worktree: true,
                    danger_level: DangerLevel::Medium,
                    ..ActionEffects::default()
                },
                ConfirmPolicy::OnDanger,
            ),
            spec(
                "branch.create",
                "Create Branch",
                Some("repo.is_open"),
                None,
                ActionEffects {
                    writes_refs: true,
                    danger_level: DangerLevel::Medium,
                    ..ActionEffects::default()
                },
                ConfirmPolicy::OnDanger,
            ),
            spec(
                "branch.rename",
                "Rename Branch",
                Some("repo.is_open"),
                None,
                ActionEffects {
                    writes_refs: true,
                    danger_level: DangerLevel::Medium,
                    ..ActionEffects::default()
                },
                ConfirmPolicy::OnDanger,
            ),
            spec(
                "branch.delete",
                "Delete Branch",
                Some("repo.is_open"),
                Some(DangerLevel::High),
                ActionEffects::mutating_refs(),
                ConfirmPolicy::Always,
            ),
            spec(
                "rebase.interactive",
                "Interactive Rebase",
                Some("repo.is_open"),
                Some(DangerLevel::High),
                ActionEffects {
                    writes_refs: true,
                    writes_index: true,
                    writes_worktree: true,
                    danger_level: DangerLevel::High,
                    ..ActionEffects::default()
                },
                ConfirmPolicy::Always,
            ),
            spec(
                "rebase.plan.create",
                "Create Rebase Plan",
                Some("repo.is_open"),
                Some(DangerLevel::Medium),
                ActionEffects::read_only(),
                ConfirmPolicy::OnDanger,
            ),
            spec(
                "rebase.plan.set_action",
                "Set Rebase Plan Action",
                Some("repo.is_open"),
                Some(DangerLevel::Low),
                ActionEffects::read_only(),
                ConfirmPolicy::Never,
            ),
            spec(
                "rebase.plan.move",
                "Reorder Rebase Plan Entry",
                Some("repo.is_open"),
                Some(DangerLevel::Low),
                ActionEffects::read_only(),
                ConfirmPolicy::Never,
            ),
            spec(
                "rebase.plan.clear",
                "Clear Rebase Plan",
                Some("repo.is_open"),
                Some(DangerLevel::Low),
                ActionEffects::read_only(),
                ConfirmPolicy::Never,
            ),
            spec(
                "rebase.execute",
                "Execute Rebase Plan",
                Some("repo.is_open"),
                Some(DangerLevel::High),
                ActionEffects {
                    writes_refs: true,
                    writes_index: true,
                    writes_worktree: true,
                    danger_level: DangerLevel::High,
                    ..ActionEffects::default()
                },
                ConfirmPolicy::Always,
            ),
            spec(
                "rebase.continue",
                "Continue Rebase",
                Some("repo.is_open"),
                Some(DangerLevel::Medium),
                ActionEffects {
                    writes_refs: true,
                    writes_index: true,
                    writes_worktree: true,
                    danger_level: DangerLevel::Medium,
                    ..ActionEffects::default()
                },
                ConfirmPolicy::OnDanger,
            ),
            spec(
                "rebase.skip",
                "Skip Rebase Commit",
                Some("repo.is_open"),
                Some(DangerLevel::Medium),
                ActionEffects {
                    writes_refs: true,
                    writes_index: true,
                    writes_worktree: true,
                    danger_level: DangerLevel::Medium,
                    ..ActionEffects::default()
                },
                ConfirmPolicy::OnDanger,
            ),
            spec(
                "rebase.abort",
                "Abort Rebase",
                Some("repo.is_open"),
                Some(DangerLevel::Medium),
                ActionEffects {
                    writes_refs: true,
                    writes_index: true,
                    writes_worktree: true,
                    danger_level: DangerLevel::Medium,
                    ..ActionEffects::default()
                },
                ConfirmPolicy::OnDanger,
            ),
            spec(
                "merge.execute",
                "Merge Branch",
                Some("repo.is_open"),
                Some(DangerLevel::High),
                ActionEffects {
                    writes_refs: true,
                    writes_index: true,
                    writes_worktree: true,
                    danger_level: DangerLevel::High,
                    ..ActionEffects::default()
                },
                ConfirmPolicy::Always,
            ),
            spec(
                "merge.abort",
                "Abort Merge",
                Some("repo.is_open"),
                Some(DangerLevel::Medium),
                ActionEffects {
                    writes_refs: true,
                    writes_index: true,
                    writes_worktree: true,
                    danger_level: DangerLevel::Medium,
                    ..ActionEffects::default()
                },
                ConfirmPolicy::OnDanger,
            ),
            spec(
                "conflict.list",
                "List Conflicts",
                Some("repo.is_open"),
                Some(DangerLevel::Low),
                ActionEffects::read_only(),
                ConfirmPolicy::Never,
            ),
            spec(
                "conflict.focus",
                "Focus Conflict File",
                Some("repo.is_open"),
                Some(DangerLevel::Low),
                ActionEffects::read_only(),
                ConfirmPolicy::Never,
            ),
            spec(
                "conflict.resolve.ours",
                "Resolve Conflict (Ours)",
                Some("repo.is_open"),
                Some(DangerLevel::Medium),
                ActionEffects::mutating_worktree(),
                ConfirmPolicy::OnDanger,
            ),
            spec(
                "conflict.resolve.theirs",
                "Resolve Conflict (Theirs)",
                Some("repo.is_open"),
                Some(DangerLevel::Medium),
                ActionEffects::mutating_worktree(),
                ConfirmPolicy::OnDanger,
            ),
            spec(
                "conflict.mark_resolved",
                "Mark Conflict Resolved",
                Some("repo.is_open"),
                Some(DangerLevel::Medium),
                ActionEffects {
                    writes_index: true,
                    danger_level: DangerLevel::Medium,
                    ..ActionEffects::default()
                },
                ConfirmPolicy::OnDanger,
            ),
            spec(
                "conflict.continue",
                "Continue Conflict Session",
                Some("repo.is_open"),
                Some(DangerLevel::Medium),
                ActionEffects {
                    writes_refs: true,
                    writes_index: true,
                    writes_worktree: true,
                    danger_level: DangerLevel::Medium,
                    ..ActionEffects::default()
                },
                ConfirmPolicy::OnDanger,
            ),
            spec(
                "conflict.abort",
                "Abort Conflict Session",
                Some("repo.is_open"),
                Some(DangerLevel::Medium),
                ActionEffects {
                    writes_refs: true,
                    writes_index: true,
                    writes_worktree: true,
                    danger_level: DangerLevel::Medium,
                    ..ActionEffects::default()
                },
                ConfirmPolicy::OnDanger,
            ),
            spec(
                "reset.soft",
                "Reset --soft",
                Some("repo.is_open"),
                Some(DangerLevel::Medium),
                ActionEffects {
                    writes_refs: true,
                    danger_level: DangerLevel::Medium,
                    ..ActionEffects::default()
                },
                ConfirmPolicy::OnDanger,
            ),
            spec(
                "reset.mixed",
                "Reset --mixed",
                Some("repo.is_open"),
                Some(DangerLevel::High),
                ActionEffects {
                    writes_refs: true,
                    writes_index: true,
                    danger_level: DangerLevel::High,
                    ..ActionEffects::default()
                },
                ConfirmPolicy::Always,
            ),
            spec(
                "reset.hard",
                "Reset --hard",
                Some("repo.is_open"),
                Some(DangerLevel::High),
                ActionEffects {
                    writes_refs: true,
                    writes_index: true,
                    writes_worktree: true,
                    danger_level: DangerLevel::High,
                    ..ActionEffects::default()
                },
                ConfirmPolicy::Always,
            ),
        ],
        views: vec![plugin_api::ViewSpec {
            view_id: "branches.panel".to_string(),
            title: "Branches".to_string(),
            slot: "left".to_string(),
            when: Some("repo.is_open".to_string()),
        }],
    }
}

pub fn tags_registration_payload() -> PluginRegister {
    PluginRegister {
        actions: vec![
            spec(
                "tag.create",
                "Create Tag",
                Some("repo.is_open"),
                Some(DangerLevel::Low),
                ActionEffects {
                    writes_refs: true,
                    danger_level: DangerLevel::Low,
                    ..ActionEffects::default()
                },
                ConfirmPolicy::Never,
            ),
            spec(
                "tag.delete",
                "Delete Tag",
                Some("repo.is_open"),
                Some(DangerLevel::Medium),
                ActionEffects {
                    writes_refs: true,
                    danger_level: DangerLevel::Medium,
                    ..ActionEffects::default()
                },
                ConfirmPolicy::OnDanger,
            ),
            spec(
                "tag.checkout",
                "Checkout Tag",
                Some("repo.is_open"),
                Some(DangerLevel::Medium),
                ActionEffects {
                    writes_refs: true,
                    writes_worktree: true,
                    danger_level: DangerLevel::Medium,
                    ..ActionEffects::default()
                },
                ConfirmPolicy::OnDanger,
            ),
        ],
        views: vec![plugin_api::ViewSpec {
            view_id: "tags.panel".to_string(),
            title: "Tags".to_string(),
            slot: "left".to_string(),
            when: Some("repo.is_open".to_string()),
        }],
    }
}

pub fn compare_registration_payload() -> PluginRegister {
    PluginRegister {
        actions: vec![spec(
            "compare.refs",
            "Compare Branches",
            Some("repo.is_open"),
            None,
            ActionEffects::read_only(),
            ConfirmPolicy::Never,
        )],
        views: vec![plugin_api::ViewSpec {
            view_id: "compare.panel".to_string(),
            title: "Compare".to_string(),
            slot: "left".to_string(),
            when: Some("repo.is_open".to_string()),
        }],
    }
}

pub fn diagnostics_registration_payload() -> PluginRegister {
    PluginRegister {
        actions: vec![
            spec(
                "diagnostics.journal_summary",
                "Show Journal Summary",
                Some("repo.is_open"),
                None,
                ActionEffects::read_only(),
                ConfirmPolicy::Never,
            ),
            spec(
                "diagnostics.repo_capabilities",
                "Show Repo Capabilities",
                Some("repo.is_open"),
                None,
                ActionEffects::read_only(),
                ConfirmPolicy::Never,
            ),
            spec(
                "diagnostics.lfs_status",
                "Show LFS Status",
                Some("repo.is_open"),
                None,
                ActionEffects::read_only(),
                ConfirmPolicy::Never,
            ),
            spec(
                "diagnostics.lfs_fetch",
                "Fetch LFS Objects",
                Some("repo.is_open"),
                Some(DangerLevel::Low),
                ActionEffects {
                    network: true,
                    danger_level: DangerLevel::Low,
                    ..ActionEffects::default()
                },
                ConfirmPolicy::Never,
            ),
            spec(
                "diagnostics.lfs_pull",
                "Pull LFS Objects",
                Some("repo.is_open"),
                Some(DangerLevel::Low),
                ActionEffects {
                    network: true,
                    danger_level: DangerLevel::Low,
                    ..ActionEffects::default()
                },
                ConfirmPolicy::Never,
            ),
        ],
        views: vec![plugin_api::ViewSpec {
            view_id: "diagnostics.panel".to_string(),
            title: "Diagnostics".to_string(),
            slot: "right".to_string(),
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
        assert_eq!(session.lifecycle_state(), PluginLifecycleState::Discovered);
        let hello = PluginHello {
            plugin_id: "status".to_string(),
            version: "0.1".to_string(),
        };
        let ack = session.handle_hello(&hello);
        assert!(ack.is_ok());
        assert_eq!(session.lifecycle_state(), PluginLifecycleState::Handshaking);

        let register = default_registration_payload();
        let register_ack = session.handle_register(&register);
        assert!(register_ack.is_ok());
        assert_eq!(session.lifecycle_state(), PluginLifecycleState::Ready);
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
        assert!(actions.iter().any(|a| a.action_id == "repo.open"));
    }

    #[test]
    fn runtime_session_rejects_invoke_before_ready() {
        let mut session = RuntimeSession::new("status");
        let result = session.invoke_action(
            "repo.open",
            ActionContext {
                selection_files: Vec::new(),
            },
            Instant::now(),
        );

        assert!(matches!(
            result,
            Err(SessionError::PluginNotReady {
                plugin_id,
                state: PluginLifecycleState::Discovered,
            }) if plugin_id == "status"
        ));
    }

    #[test]
    fn runtime_session_rejects_invalid_lifecycle_transition() {
        let mut session = RuntimeSession::new("status");
        let hello = PluginHello {
            plugin_id: "status".to_string(),
            version: "0.1".to_string(),
        };
        assert!(session.handle_hello(&hello).is_ok());

        // Повторный hello не допускается из состояния handshaking.
        let second = session.handle_hello(&hello);
        assert!(matches!(
            second,
            Err(SessionError::InvalidLifecycleTransition {
                from: PluginLifecycleState::Handshaking,
                to: PluginLifecycleState::Starting,
            })
        ));
    }

    #[test]
    fn structured_runtime_log_contains_required_fields() {
        let encoded = format_runtime_log_event(
            "status",
            "info",
            "runtime.lifecycle.transition",
            serde_json::json!({"from": "Discovered", "to": "Starting"}),
        );
        let decoded: serde_json::Value = serde_json::from_str(&encoded).expect("json");

        assert_eq!(decoded["level"], "info");
        assert_eq!(decoded["source"], "plugin_host");
        assert_eq!(decoded["plugin_id"], "status");
        assert_eq!(decoded["event"], "runtime.lifecycle.transition");
        assert_eq!(decoded["fields"]["from"], "Discovered");
        assert_eq!(decoded["fields"]["to"], "Starting");
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
        let health = supervisor.poll().expect("poll");
        assert!(matches!(health, ProcessHealth::Exited { exit_code: 1 }));
        assert!(matches!(
            supervisor.availability(),
            PluginAvailability::Unavailable { .. }
        ));
        assert_eq!(supervisor.last_exit_code(), Some(1));
        assert_eq!(supervisor.plugin_id(), "status");
    }

    #[test]
    fn supervisor_marks_temporarily_unavailable_while_restarting() {
        let config = PluginProcessConfig {
            plugin_id: "status".to_string(),
            program: "sh".to_string(),
            args: vec!["-c".to_string(), "exit 0".to_string()],
            restart_policy: RestartPolicy::Once,
        };

        let process = PluginProcess::spawn(config).expect("spawn");
        let mut supervisor = PluginSupervisor::new(process);

        std::thread::sleep(std::time::Duration::from_millis(20));
        let first = supervisor.poll().expect("poll");
        assert!(matches!(first, ProcessHealth::Restarted { exit_code: 0 }));
        assert!(matches!(
            supervisor.availability(),
            PluginAvailability::Unavailable { reason }
                if reason.contains("restarting")
        ));

        std::thread::sleep(std::time::Duration::from_millis(20));
        let second = supervisor.poll().expect("poll");
        assert!(matches!(second, ProcessHealth::Exited { exit_code: 0 }));
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
                .any(|a| a.action_id == "index.stage_lines")
        );
        assert!(
            payload
                .actions
                .iter()
                .any(|a| a.action_id == "index.unstage_lines")
        );
        assert!(
            payload
                .actions
                .iter()
                .any(|a| a.action_id == "commit.amend")
        );
        assert!(
            payload
                .actions
                .iter()
                .any(|a| a.action_id == "file.discard_hunk")
        );
        assert!(
            payload
                .actions
                .iter()
                .any(|a| a.action_id == "file.discard_lines")
        );
        assert!(
            payload
                .actions
                .iter()
                .any(|a| a.action_id == "stash.create")
        );
        assert!(payload.actions.iter().any(|a| a.action_id == "stash.list"));
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
                .any(|a| a.action_id == "history.file")
        );
        assert!(payload.actions.iter().any(|a| a.action_id == "blame.file"));
        assert!(
            payload
                .actions
                .iter()
                .any(|a| a.action_id == "history.clear_filter")
        );
        assert!(
            payload
                .actions
                .iter()
                .any(|a| a.action_id == "cherry_pick.commit")
        );
        assert!(
            payload
                .actions
                .iter()
                .any(|a| a.action_id == "revert.commit")
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
                .any(|a| a.action_id == "merge.execute")
        );
        assert!(
            payload
                .actions
                .iter()
                .any(|a| a.action_id == "rebase.plan.create")
        );
        assert!(
            payload
                .actions
                .iter()
                .any(|a| a.action_id == "rebase.plan.set_action")
        );
        assert!(
            payload
                .actions
                .iter()
                .any(|a| a.action_id == "rebase.plan.move")
        );
        assert!(
            payload
                .actions
                .iter()
                .any(|a| a.action_id == "rebase.plan.clear")
        );
        assert!(
            payload
                .actions
                .iter()
                .any(|a| a.action_id == "rebase.execute")
        );
        assert!(
            payload
                .actions
                .iter()
                .any(|a| a.action_id == "rebase.continue")
        );
        assert!(payload.actions.iter().any(|a| a.action_id == "rebase.skip"));
        assert!(
            payload
                .actions
                .iter()
                .any(|a| a.action_id == "rebase.abort")
        );
        assert!(payload.actions.iter().any(|a| a.action_id == "merge.abort"));
        assert!(payload.actions.iter().any(|a| a.action_id == "reset.soft"));
        assert!(payload.actions.iter().any(|a| a.action_id == "reset.mixed"));
        assert!(payload.actions.iter().any(|a| a.action_id == "reset.hard"));
        assert!(
            payload
                .actions
                .iter()
                .any(|a| a.action_id == "conflict.focus")
        );
        assert!(payload.views.iter().any(|v| v.view_id == "branches.panel"));
    }

    #[test]
    fn tags_registration_payload_contains_view_and_actions() {
        let payload = tags_registration_payload();
        assert!(payload.actions.iter().any(|a| a.action_id == "tag.create"));
        assert!(payload.actions.iter().any(|a| a.action_id == "tag.delete"));
        assert!(
            payload
                .actions
                .iter()
                .any(|a| a.action_id == "tag.checkout")
        );
        assert!(payload.views.iter().any(|v| v.view_id == "tags.panel"));
    }

    #[test]
    fn compare_registration_payload_contains_view_and_actions() {
        let payload = compare_registration_payload();
        assert!(
            payload
                .actions
                .iter()
                .any(|a| a.action_id == "compare.refs")
        );
        assert!(payload.views.iter().any(|v| v.view_id == "compare.panel"));
    }

    #[test]
    fn diagnostics_registration_payload_contains_view_and_actions() {
        let payload = diagnostics_registration_payload();
        assert!(
            payload
                .actions
                .iter()
                .any(|a| a.action_id == "diagnostics.journal_summary")
        );
        assert!(
            payload
                .actions
                .iter()
                .any(|a| a.action_id == "diagnostics.repo_capabilities")
        );
        assert!(
            payload
                .actions
                .iter()
                .any(|a| a.action_id == "diagnostics.lfs_status")
        );
        assert!(
            payload
                .actions
                .iter()
                .any(|a| a.action_id == "diagnostics.lfs_fetch")
        );
        assert!(
            payload
                .actions
                .iter()
                .any(|a| a.action_id == "diagnostics.lfs_pull")
        );
        assert!(
            payload
                .views
                .iter()
                .any(|v| v.view_id == "diagnostics.panel")
        );
    }

    #[test]
    fn repo_manager_payload_contains_advanced_repo_actions() {
        let payload = repo_manager_registration_payload();
        assert!(
            payload
                .actions
                .iter()
                .any(|a| a.action_id == "worktree.list")
        );
        assert!(
            payload
                .actions
                .iter()
                .any(|a| a.action_id == "worktree.create")
        );
        assert!(
            payload
                .actions
                .iter()
                .any(|a| a.action_id == "submodule.list")
        );
    }

    #[test]
    fn plugin_manager_install_enable_disable_remove_flow() {
        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_nanos())
            .unwrap_or(0);
        let root = std::env::temp_dir().join(format!("branchforge-plugin-mgr-{nanos}"));
        let package_dir = root.join("pkg");
        let plugins_root = root.join("installed");
        assert!(std::fs::create_dir_all(&package_dir).is_ok());

        let binary = package_dir.join("sample_external_plugin");
        assert!(std::fs::write(&binary, "#!/usr/bin/env sh\nexit 0\n").is_ok());

        let manifest = plugin_api::PluginManifestV1 {
            manifest_version: plugin_api::PLUGIN_MANIFEST_VERSION_V1.to_string(),
            plugin_id: "sample_external".to_string(),
            version: "0.1.0".to_string(),
            protocol_version: plugin_api::HOST_PLUGIN_PROTOCOL_VERSION.to_string(),
            entrypoint: "sample_external_plugin".to_string(),
            description: Some("Sample plugin".to_string()),
            permissions: vec!["read_state".to_string()],
        };
        let raw = serde_json::to_string_pretty(&manifest).unwrap_or_else(|_| "{}".to_string());
        assert!(std::fs::write(package_dir.join("plugin.json"), raw).is_ok());

        let installed = install_local_plugin(&package_dir, &plugins_root);
        assert!(installed.is_ok());
        let installed = installed.expect("install");
        assert!(installed.enabled);
        assert_eq!(installed.manifest.plugin_id, "sample_external");

        let disabled = set_plugin_enabled(&plugins_root, "sample_external", false);
        assert!(disabled.is_ok());
        let disabled = disabled.expect("disable");
        assert!(!disabled.enabled);

        let listed = list_installed_plugins(&plugins_root).expect("list");
        assert_eq!(listed.len(), 1);
        assert!(!listed[0].enabled);

        let enabled = set_plugin_enabled(&plugins_root, "sample_external", true);
        assert!(enabled.is_ok());
        assert!(enabled.expect("enable").enabled);

        assert!(remove_local_plugin(&plugins_root, "sample_external").is_ok());
        let listed = list_installed_plugins(&plugins_root).expect("list");
        assert!(listed.is_empty());

        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn plugin_registry_discovery_and_install_flow() {
        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_nanos())
            .unwrap_or(0);
        let root = std::env::temp_dir().join(format!("branchforge-plugin-registry-{nanos}"));
        let registry_root = root.join("registry");
        let package_dir = root.join("packages/sample_external");
        let plugins_root = root.join("installed");
        assert!(std::fs::create_dir_all(&registry_root).is_ok());
        assert!(std::fs::create_dir_all(&package_dir).is_ok());

        let binary = package_dir.join("sample_external_plugin");
        assert!(std::fs::write(&binary, "#!/usr/bin/env sh\nexit 0\n").is_ok());
        let manifest = plugin_api::PluginManifestV1 {
            manifest_version: plugin_api::PLUGIN_MANIFEST_VERSION_V1.to_string(),
            plugin_id: "sample_external".to_string(),
            version: "0.1.0".to_string(),
            protocol_version: plugin_api::HOST_PLUGIN_PROTOCOL_VERSION.to_string(),
            entrypoint: "sample_external_plugin".to_string(),
            description: Some("Sample plugin".to_string()),
            permissions: vec!["read_state".to_string()],
        };
        let raw = serde_json::to_string_pretty(&manifest).unwrap_or_else(|_| "{}".to_string());
        assert!(std::fs::write(package_dir.join("plugin.json"), raw).is_ok());
        assert!(
            std::fs::write(
                registry_root.join("registry.json"),
                serde_json::json!({
                    "registry_version": "1",
                    "plugins": [{
                        "plugin_id": "sample_external",
                        "package_dir": "../packages/sample_external",
                        "summary": "Sample external plugin",
                        "channel": "stable"
                    }]
                })
                .to_string(),
            )
            .is_ok()
        );

        let discovered = discover_local_plugins(&registry_root).expect("discover");
        assert_eq!(discovered.len(), 1);
        assert_eq!(discovered[0].manifest.plugin_id, "sample_external");
        assert_eq!(discovered[0].channel.as_deref(), Some("stable"));

        let installed = install_registry_plugin(&registry_root, &plugins_root, "sample_external")
            .expect("install");
        assert_eq!(installed.manifest.plugin_id, "sample_external");
        assert!(installed.enabled);

        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn workspace_sample_external_package_is_installable() {
        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_nanos())
            .unwrap_or(0);
        let root = std::env::temp_dir().join(format!("branchforge-plugin-sample-{nanos}"));
        let plugins_root = root.join("installed");
        let package_dir = Path::new(env!("CARGO_MANIFEST_DIR"))
            .parent()
            .and_then(Path::parent)
            .expect("workspace root")
            .join("external_plugins/sample_plugin");

        let manifest = read_manifest(&package_dir);
        assert!(manifest.is_ok());

        let installed = install_local_plugin(&package_dir, &plugins_root);
        assert!(installed.is_ok());
        assert_eq!(
            installed.expect("install").manifest.plugin_id,
            "sample_external"
        );

        let _ = std::fs::remove_dir_all(root);
    }

    fn serve_registry_files(
        files: std::collections::HashMap<String, Vec<u8>>,
        request_count: usize,
    ) -> Option<(String, std::thread::JoinHandle<()>)> {
        let listener = match std::net::TcpListener::bind("127.0.0.1:0") {
            Ok(listener) => listener,
            Err(err) if err.kind() == std::io::ErrorKind::PermissionDenied => return None,
            Err(err) => panic!("bind test server: {err}"),
        };
        let address = listener.local_addr().expect("server addr");
        let handle = std::thread::spawn(move || {
            for _ in 0..request_count {
                let (mut stream, _) = listener.accept().expect("accept");
                let mut buffer = [0u8; 4096];
                let read = stream.read(&mut buffer).expect("read request");
                let request = String::from_utf8_lossy(&buffer[..read]);
                let path = request
                    .lines()
                    .next()
                    .and_then(|line| line.split_whitespace().nth(1))
                    .unwrap_or("/");
                let body = files.get(path).cloned().unwrap_or_default();
                let status = if files.contains_key(path) {
                    "HTTP/1.1 200 OK"
                } else {
                    "HTTP/1.1 404 Not Found"
                };
                let response = format!(
                    "{status}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                    body.len()
                );
                stream
                    .write_all(response.as_bytes())
                    .and_then(|_| stream.write_all(&body))
                    .expect("write response");
            }
        });

        Some((format!("http://{}", address), handle))
    }

    #[test]
    fn plugin_registry_remote_http_discovery_and_install_flow() {
        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_nanos())
            .unwrap_or(0);
        let root = std::env::temp_dir().join(format!("branchforge-plugin-remote-registry-{nanos}"));
        let plugins_root = root.join("installed");
        let manifest = plugin_api::PluginManifestV1 {
            manifest_version: plugin_api::PLUGIN_MANIFEST_VERSION_V1.to_string(),
            plugin_id: "remote_sample".to_string(),
            version: "0.1.0".to_string(),
            protocol_version: plugin_api::HOST_PLUGIN_PROTOCOL_VERSION.to_string(),
            entrypoint: "remote_sample_plugin".to_string(),
            description: Some("Remote sample plugin".to_string()),
            permissions: vec!["read_state".to_string()],
        };
        let manifest_raw = serde_json::to_vec_pretty(&manifest).expect("manifest json");
        let registry_raw = serde_json::json!({
            "registry_version": "1",
            "plugins": [{
                "plugin_id": "remote_sample",
                "manifest_url": "plugin.json",
                "entrypoint_url": "remote_sample_plugin",
                "summary": "Remote sample plugin",
                "channel": "stable"
            }]
        })
        .to_string()
        .into_bytes();

        let mut files = std::collections::HashMap::new();
        files.insert("/registry.json".to_string(), registry_raw);
        files.insert("/plugin.json".to_string(), manifest_raw);
        files.insert(
            "/remote_sample_plugin".to_string(),
            b"#!/usr/bin/env sh\nexit 0\n".to_vec(),
        );
        let Some((base_url, server)) = serve_registry_files(files, 5) else {
            return;
        };
        let registry_url = format!("{base_url}/registry.json");

        let discovered =
            discover_local_plugins(Path::new(&registry_url)).expect("discover remote registry");
        assert_eq!(discovered.len(), 1);
        assert_eq!(discovered[0].manifest.plugin_id, "remote_sample");
        let expected_manifest_url = format!("{base_url}/plugin.json");
        assert_eq!(
            discovered[0].manifest_url.as_deref(),
            Some(expected_manifest_url.as_str())
        );

        let installed =
            install_registry_plugin(Path::new(&registry_url), &plugins_root, "remote_sample")
                .expect("install remote registry plugin");
        assert_eq!(installed.manifest.plugin_id, "remote_sample");
        assert!(plugins_root.join("remote_sample/plugin.json").exists());
        assert!(
            plugins_root
                .join("remote_sample/remote_sample_plugin")
                .exists()
        );

        server.join().expect("join test server");
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn plugin_manager_rejects_incompatible_manifest() {
        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_nanos())
            .unwrap_or(0);
        let root = std::env::temp_dir().join(format!("branchforge-plugin-mgr-bad-{nanos}"));
        let package_dir = root.join("pkg");
        let plugins_root = root.join("installed");
        assert!(std::fs::create_dir_all(&package_dir).is_ok());

        assert!(std::fs::write(package_dir.join("bin"), "binary").is_ok());
        let manifest = plugin_api::PluginManifestV1 {
            manifest_version: plugin_api::PLUGIN_MANIFEST_VERSION_V1.to_string(),
            plugin_id: "bad_plugin".to_string(),
            version: "0.1.0".to_string(),
            protocol_version: "9.9".to_string(),
            entrypoint: "bin".to_string(),
            description: None,
            permissions: Vec::new(),
        };
        let raw = serde_json::to_string_pretty(&manifest).unwrap_or_else(|_| "{}".to_string());
        assert!(std::fs::write(package_dir.join("plugin.json"), raw).is_ok());

        let installed = install_local_plugin(&package_dir, &plugins_root);
        assert!(matches!(
            installed,
            Err(PluginManagerError::IncompatiblePlugin { .. })
        ));

        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn branches_payload_includes_interactive_rebase() {
        let payload = branches_registration_payload();
        assert!(
            payload
                .actions
                .iter()
                .any(|a| a.action_id == "rebase.interactive")
        );
    }
}
