use std::collections::HashMap;
use std::io::{BufRead, BufReader, Read, Write};
use std::net::{TcpListener, TcpStream};

use app_host::{ConsoleRunnerConfig, HostRuntime, HostRuntimeError};
use state_store::{DiffSource, JournalStatus, PluginHealth, StoreSnapshot};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GuiServerConfig {
    pub bind: String,
}

impl Default for GuiServerConfig {
    fn default() -> Self {
        Self {
            bind: "127.0.0.1:8787".to_string(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct HttpRequest {
    method: String,
    path: String,
    body: Vec<u8>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct HttpResponse {
    status: &'static str,
    content_type: &'static str,
    body: String,
}

pub fn run_gui_server(config: GuiServerConfig) -> Result<(), String> {
    let listener =
        TcpListener::bind(&config.bind).map_err(|err| format!("bind {}: {}", config.bind, err))?;
    let runtime_config = ConsoleRunnerConfig::from_current_env()?;
    let mut runtime = HostRuntime::new(runtime_config);

    println!("Branchforge GUI listening on http://{}", config.bind);
    println!("Open that URL in your browser.");

    for stream in listener.incoming() {
        match stream {
            Ok(mut stream) => {
                if let Err(err) = handle_connection(&mut stream, &mut runtime) {
                    let _ = write_http_response(
                        &mut stream,
                        HttpResponse {
                            status: "500 Internal Server Error",
                            content_type: "text/plain; charset=utf-8",
                            body: format!("server error: {err}"),
                        },
                    );
                }
            }
            Err(err) => return Err(format!("accept failed: {err}")),
        }
    }

    Ok(())
}

fn handle_connection(stream: &mut TcpStream, runtime: &mut HostRuntime) -> Result<(), String> {
    let request = read_http_request(stream)?;
    let response = route_request(runtime, request);
    write_http_response(stream, response)
}

fn route_request(runtime: &mut HostRuntime, request: HttpRequest) -> HttpResponse {
    match (request.method.as_str(), request.path.as_str()) {
        ("GET", "/") => ok_html(render_page(runtime, None, None)),
        ("POST", "/command") => {
            let form = parse_form_urlencoded(&request.body);
            let command = build_command_from_form(&form);
            match command {
                Some(command) => match runtime.submit_line(&command) {
                    Ok(message) => ok_html(render_page(runtime, message.as_deref(), None)),
                    Err(error) => ok_html(render_page(runtime, None, Some(&error))),
                },
                None => ok_html(render_page(
                    runtime,
                    None,
                    Some(&HostRuntimeError {
                        title: "Invalid input".to_string(),
                        message: "No command was submitted.".to_string(),
                        detail: None,
                    }),
                )),
            }
        }
        _ => HttpResponse {
            status: "404 Not Found",
            content_type: "text/plain; charset=utf-8",
            body: "not found".to_string(),
        },
    }
}

fn ok_html(body: String) -> HttpResponse {
    HttpResponse {
        status: "200 OK",
        content_type: "text/html; charset=utf-8",
        body,
    }
}

fn read_http_request(stream: &mut TcpStream) -> Result<HttpRequest, String> {
    let clone = stream.try_clone().map_err(|err| err.to_string())?;
    let mut reader = BufReader::new(clone);
    let mut request_line = String::new();
    reader
        .read_line(&mut request_line)
        .map_err(|err| err.to_string())?;
    if request_line.trim().is_empty() {
        return Err("empty request".to_string());
    }
    let mut parts = request_line.split_whitespace();
    let method = parts
        .next()
        .ok_or_else(|| "missing method".to_string())?
        .to_string();
    let path = parts
        .next()
        .ok_or_else(|| "missing path".to_string())?
        .split('?')
        .next()
        .unwrap_or("/")
        .to_string();

    let mut content_length = 0usize;
    loop {
        let mut line = String::new();
        reader.read_line(&mut line).map_err(|err| err.to_string())?;
        if line == "\r\n" || line.is_empty() {
            break;
        }
        if let Some(value) = parse_content_length_header(&line) {
            content_length = value;
        }
    }

    let mut body = vec![0u8; content_length];
    if content_length > 0 {
        reader
            .read_exact(&mut body)
            .map_err(|err| err.to_string())?;
    }

    Ok(HttpRequest { method, path, body })
}

fn parse_content_length_header(line: &str) -> Option<usize> {
    let (name, value) = line.split_once(':')?;
    if name.eq_ignore_ascii_case("content-length") {
        Some(value.trim().parse::<usize>().unwrap_or(0))
    } else {
        None
    }
}

fn write_http_response(stream: &mut TcpStream, response: HttpResponse) -> Result<(), String> {
    let bytes = response.body.as_bytes();
    write!(
        stream,
        "HTTP/1.1 {}\r\nContent-Type: {}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
        response.status,
        response.content_type,
        bytes.len()
    )
    .map_err(|err| err.to_string())?;
    stream.write_all(bytes).map_err(|err| err.to_string())
}

fn build_command_from_form(form: &HashMap<String, String>) -> Option<String> {
    if let Some(action) = form_value(form, "gui_action") {
        return build_gui_action_command(action, form);
    }

    if let Some(command) = form_value(form, "command") {
        return Some(command.to_string());
    }

    form_value(form, "repo_path").map(|value| format!("open {}", shell_quote(value)))
}

fn build_gui_action_command(action: &str, form: &HashMap<String, String>) -> Option<String> {
    match action {
        "repo_open" => Some(build_run_command(
            "repo.open",
            vec![form_value(form, "repo_path")?.to_string()],
            false,
        )),
        "commit_create" => Some(build_run_command(
            "commit.create",
            vec![form_value(form, "commit_message")?.to_string()],
            false,
        )),
        "commit_amend" => Some(build_run_command(
            "commit.amend",
            vec![form_value(form, "commit_message")?.to_string()],
            false,
        )),
        "branch_create" => {
            let mut args = vec![form_value(form, "branch_name")?.to_string()];
            if let Some(base) = form_value(form, "branch_base") {
                args.push(base.to_string());
            }
            Some(build_run_command("branch.create", args, false))
        }
        "branch_checkout" => Some(build_run_command(
            "branch.checkout",
            form_value(form, "branch_name")
                .map(|value| vec![value.to_string()])
                .unwrap_or_default(),
            false,
        )),
        "branch_rename" => {
            let mut args = Vec::new();
            if let Some(old_name) = form_value(form, "branch_old_name") {
                args.push(old_name.to_string());
            }
            args.push(form_value(form, "branch_new_name")?.to_string());
            Some(build_run_command("branch.rename", args, false))
        }
        "branch_delete" => Some(build_run_command(
            "branch.delete",
            vec![form_value(form, "branch_name")?.to_string()],
            true,
        )),
        "tag_create" => {
            let mut args = vec![form_value(form, "tag_name")?.to_string()];
            if let Some(target) = form_value(form, "tag_target") {
                args.push(target.to_string());
            }
            Some(build_run_command("tag.create", args, false))
        }
        "tag_checkout" => Some(build_run_command(
            "tag.checkout",
            vec![form_value(form, "tag_name")?.to_string()],
            false,
        )),
        "tag_delete" => Some(build_run_command(
            "tag.delete",
            vec![form_value(form, "tag_name")?.to_string()],
            true,
        )),
        "compare_refs" => Some(build_run_command(
            "compare.refs",
            vec![
                form_value(form, "compare_base")?.to_string(),
                form_value(form, "compare_head")?.to_string(),
            ],
            false,
        )),
        "history_search" => Some(build_run_command(
            "history.search",
            history_filter_args(form),
            false,
        )),
        "history_filter" => Some(build_run_command(
            "history.page",
            history_filter_args(form),
            false,
        )),
        "history_file" => Some(build_run_command(
            "history.file",
            history_file_args(form),
            false,
        )),
        "blame_file" => Some(build_run_command(
            "blame.file",
            optional_single_arg(form, "history_file_path"),
            false,
        )),
        "stash_create" => Some(build_run_command(
            "stash.create",
            vec![form_value(form, "stash_message")?.to_string()],
            false,
        )),
        "stash_list" => Some(build_run_command("stash.list", Vec::new(), false)),
        "stash_apply" => Some(build_run_command(
            "stash.apply",
            vec![form_value(form, "stash_selector")?.to_string()],
            false,
        )),
        "stash_pop" => Some(build_run_command(
            "stash.pop",
            vec![form_value(form, "stash_selector")?.to_string()],
            false,
        )),
        "stash_drop" => Some(build_run_command(
            "stash.drop",
            vec![form_value(form, "stash_selector")?.to_string()],
            true,
        )),
        "worktree_list" => Some(build_run_command("worktree.list", Vec::new(), false)),
        "worktree_create" => Some(build_run_command(
            "worktree.create",
            vec![
                form_value(form, "worktree_path")?.to_string(),
                form_value(form, "worktree_branch")?.to_string(),
            ],
            false,
        )),
        "worktree_open" => Some(build_run_command(
            "worktree.open",
            vec![form_value(form, "worktree_path")?.to_string()],
            false,
        )),
        "worktree_remove" => Some(build_run_command(
            "worktree.remove",
            vec![form_value(form, "worktree_path")?.to_string()],
            true,
        )),
        "submodule_list" => Some(build_run_command("submodule.list", Vec::new(), false)),
        "submodule_init_update" => Some(build_run_command(
            "submodule.init_update",
            optional_single_arg(form, "submodule_path"),
            false,
        )),
        "submodule_open" => Some(build_run_command(
            "submodule.open",
            vec![form_value(form, "submodule_path")?.to_string()],
            false,
        )),
        "merge_execute" => {
            let mut args = vec![form_value(form, "merge_source_ref")?.to_string()];
            if let Some(mode) = form_value(form, "merge_mode") {
                args.push(mode.to_string());
            }
            Some(build_run_command("merge.execute", args, true))
        }
        "merge_abort" => Some(build_run_command("merge.abort", Vec::new(), false)),
        "cherry_pick_commit" => Some(build_run_command(
            "cherry_pick.commit",
            optional_single_arg(form, "commit_oid"),
            false,
        )),
        "cherry_pick_abort" => Some(build_run_command("cherry_pick.abort", Vec::new(), false)),
        "revert_commit" => Some(build_run_command(
            "revert.commit",
            optional_single_arg(form, "commit_oid"),
            false,
        )),
        "reset_refs" => {
            let mut args = vec![form_value(form, "reset_mode")?.to_string()];
            if let Some(target) = form_value(form, "reset_target") {
                args.push(target.to_string());
            }
            Some(build_run_command("reset.refs", args, true))
        }
        "rebase_interactive" => {
            let mut args = vec![form_value(form, "rebase_base_ref")?.to_string()];
            if form.contains_key("rebase_autosquash") {
                args.push("autosquash".to_string());
            }
            Some(build_run_command("rebase.interactive", args, true))
        }
        "rebase_plan_create" => Some(build_run_command(
            "rebase.plan.create",
            vec![form_value(form, "rebase_base_ref")?.to_string()],
            false,
        )),
        "rebase_execute" => {
            let mut args = Vec::new();
            if form.contains_key("rebase_autosquash") {
                args.push("autosquash".to_string());
            }
            Some(build_run_command("rebase.execute", args, true))
        }
        "rebase_set_action" => Some(build_run_command(
            "rebase.plan.set_action",
            vec![
                form_value(form, "entry_index")?.to_string(),
                form_value(form, "rebase_action")?.to_string(),
            ],
            false,
        )),
        "rebase_move" => Some(build_run_command(
            "rebase.plan.move",
            vec![
                form_value(form, "from_index")?.to_string(),
                form_value(form, "to_index")?.to_string(),
            ],
            false,
        )),
        "rebase_clear" => Some(build_run_command("rebase.plan.clear", Vec::new(), false)),
        "rebase_continue" => Some(build_run_command("rebase.continue", Vec::new(), false)),
        "rebase_skip" => Some(build_run_command("rebase.skip", Vec::new(), false)),
        "rebase_abort" => Some(build_run_command("rebase.abort", Vec::new(), false)),
        "conflict_list" => Some(build_run_command("conflict.list", Vec::new(), false)),
        "conflict_focus" => Some(build_run_command(
            "conflict.focus",
            optional_single_arg(form, "conflict_path"),
            false,
        )),
        "conflict_resolve_ours" => Some(build_run_command(
            "conflict.resolve.ours",
            optional_single_arg(form, "conflict_path"),
            false,
        )),
        "conflict_resolve_theirs" => Some(build_run_command(
            "conflict.resolve.theirs",
            optional_single_arg(form, "conflict_path"),
            false,
        )),
        "conflict_mark_resolved" => Some(build_run_command(
            "conflict.mark_resolved",
            optional_single_arg(form, "conflict_path"),
            false,
        )),
        "conflict_continue" => Some(build_run_command("conflict.continue", Vec::new(), false)),
        "conflict_abort" => Some(build_run_command("conflict.abort", Vec::new(), false)),
        "diagnostics_repo_capabilities" => Some(build_run_command(
            "diagnostics.repo_capabilities",
            Vec::new(),
            false,
        )),
        "diagnostics_lfs_status" => Some(build_run_command(
            "diagnostics.lfs_status",
            Vec::new(),
            false,
        )),
        "diagnostics_lfs_fetch" => Some(build_run_command(
            "diagnostics.lfs_fetch",
            Vec::new(),
            false,
        )),
        "diagnostics_lfs_pull" => {
            Some(build_run_command("diagnostics.lfs_pull", Vec::new(), false))
        }
        "plugin_list" => Some(build_run_command("plugin.list", Vec::new(), false)),
        "plugin_discover" => Some(build_run_command(
            "plugin.discover",
            optional_single_arg(form, "registry_path"),
            false,
        )),
        "plugin_install" => Some(build_run_command(
            "plugin.install",
            vec![form_value(form, "package_dir")?.to_string()],
            false,
        )),
        "plugin_install_registry" => {
            let mut args = vec![form_value(form, "plugin_id")?.to_string()];
            if let Some(registry_path) = form_value(form, "registry_path") {
                args.push(registry_path.to_string());
            }
            Some(build_run_command("plugin.install_registry", args, false))
        }
        "plugin_enable" => Some(build_run_command(
            "plugin.enable",
            optional_single_arg(form, "plugin_id"),
            false,
        )),
        "plugin_disable" => Some(build_run_command(
            "plugin.disable",
            optional_single_arg(form, "plugin_id"),
            false,
        )),
        "plugin_remove" => Some(build_run_command(
            "plugin.remove",
            optional_single_arg(form, "plugin_id"),
            true,
        )),
        "ops_check_deps" => Some(build_run_command("ops.check_deps", Vec::new(), false)),
        "ops_dev_check" => Some(build_run_command("ops.dev_check", Vec::new(), false)),
        "release_notes" => Some(build_run_command(
            "release.notes",
            release_notes_args(form),
            false,
        )),
        "release_package_local" => Some(build_run_command(
            "release.package_local",
            release_args(form),
            false,
        )),
        "release_sign" => Some(build_run_command(
            "release.sign",
            optional_single_arg(form, "release_out_dir"),
            false,
        )),
        "release_package" => Some(build_run_command(
            "release.package",
            release_args(form),
            false,
        )),
        "release_verify" => Some(build_run_command(
            "release.verify",
            release_args(form),
            false,
        )),
        "verify_sprint22" => Some(build_run_command("verify.sprint22", Vec::new(), false)),
        "verify_sprint23" => Some(build_run_command(
            "verify.sprint23",
            optional_single_arg(form, "release_out_dir"),
            false,
        )),
        "verify_sprint24" => Some(build_run_command(
            "verify.sprint24",
            release_args(form),
            false,
        )),
        "diff_hunk_stage" => Some(build_run_command(
            "index.stage_hunk",
            diff_hunk_args(form)?,
            false,
        )),
        "diff_hunk_unstage" => Some(build_run_command(
            "index.unstage_hunk",
            diff_hunk_args(form)?,
            false,
        )),
        "diff_hunk_discard" => Some(build_run_command(
            "file.discard_hunk",
            diff_hunk_args(form)?,
            true,
        )),
        "diff_lines_stage" => Some(build_run_command(
            "index.stage_lines",
            diff_line_args(form)?,
            false,
        )),
        "diff_lines_unstage" => Some(build_run_command(
            "index.unstage_lines",
            diff_line_args(form)?,
            false,
        )),
        "diff_lines_discard" => Some(build_run_command(
            "file.discard_lines",
            diff_line_args(form)?,
            true,
        )),
        _ => None,
    }
}

fn form_value<'a>(form: &'a HashMap<String, String>, key: &str) -> Option<&'a str> {
    form.get(key)
        .map(String::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
}

fn optional_single_arg(form: &HashMap<String, String>, key: &str) -> Vec<String> {
    form_value(form, key)
        .map(|value| vec![value.to_string()])
        .unwrap_or_default()
}

fn history_filter_args(form: &HashMap<String, String>) -> Vec<String> {
    let limit = form_value(form, "history_limit")
        .unwrap_or("20")
        .to_string();
    let author = form
        .get("history_author")
        .map(|value| value.trim())
        .unwrap_or("");
    let text = form
        .get("history_text")
        .map(|value| value.trim())
        .unwrap_or("");
    let hash_prefix = form_value(form, "history_hash_prefix");

    let mut args = vec!["0".to_string(), limit, author.to_string(), text.to_string()];
    if let Some(hash_prefix) = hash_prefix {
        args.push(hash_prefix.to_string());
    }
    args
}

fn history_file_args(form: &HashMap<String, String>) -> Vec<String> {
    if let Some(path) = form_value(form, "history_file_path") {
        vec![
            path.to_string(),
            "0".to_string(),
            form_value(form, "history_limit")
                .unwrap_or("20")
                .to_string(),
        ]
    } else {
        Vec::new()
    }
}

fn release_notes_args(form: &HashMap<String, String>) -> Vec<String> {
    let mut args = Vec::new();
    if let Some(out_file) = form_value(form, "release_out_dir") {
        args.push(out_file.to_string());
    }
    if let Some(channel) = form_value(form, "release_channel") {
        args.push(channel.to_string());
    }
    args
}

fn release_args(form: &HashMap<String, String>) -> Vec<String> {
    let mut args = Vec::new();
    if let Some(out_dir) = form_value(form, "release_out_dir") {
        args.push(out_dir.to_string());
        if let Some(channel) = form_value(form, "release_channel") {
            args.push(channel.to_string());
            if let Some(rollback_from) = form_value(form, "release_rollback_from") {
                args.push(rollback_from.to_string());
            }
        }
    }
    args
}

fn diff_hunk_args(form: &HashMap<String, String>) -> Option<Vec<String>> {
    Some(vec![
        form_value(form, "diff_path")?.to_string(),
        form_value(form, "diff_hunk_index")?.to_string(),
    ])
}

fn diff_line_args(form: &HashMap<String, String>) -> Option<Vec<String>> {
    let mut args = diff_hunk_args(form)?;
    let indices = parse_line_indices(form_value(form, "line_indices")?)?;
    args.extend(indices);
    Some(args)
}

fn parse_line_indices(value: &str) -> Option<Vec<String>> {
    let indices = value
        .split(|ch: char| ch == ',' || ch.is_whitespace())
        .filter(|part| !part.is_empty())
        .map(str::trim)
        .map(|part| {
            if part.chars().all(|ch| ch.is_ascii_digit()) {
                Some(part.to_string())
            } else {
                None
            }
        })
        .collect::<Option<Vec<_>>>()?;
    if indices.is_empty() {
        None
    } else {
        Some(indices)
    }
}

fn build_run_command(op: &str, args: Vec<String>, confirm: bool) -> String {
    let mut parts = vec!["run".to_string()];
    if confirm {
        parts.push("--confirm".to_string());
    }
    parts.push(op.to_string());
    parts.extend(args.into_iter().map(|arg| shell_quote(&arg)));
    parts.join(" ")
}

fn parse_form_urlencoded(body: &[u8]) -> HashMap<String, String> {
    let mut values = HashMap::new();
    let raw = String::from_utf8_lossy(body);
    for pair in raw.split('&') {
        if pair.is_empty() {
            continue;
        }
        let mut parts = pair.splitn(2, '=');
        let key = url_decode(parts.next().unwrap_or_default());
        let value = url_decode(parts.next().unwrap_or_default());
        values.insert(key, value);
    }
    values
}

fn url_decode(value: &str) -> String {
    let mut bytes = Vec::with_capacity(value.len());
    let raw = value.as_bytes();
    let mut index = 0usize;
    while index < raw.len() {
        match raw[index] {
            b'+' => {
                bytes.push(b' ');
                index += 1;
            }
            b'%' if index + 2 < raw.len() => {
                let hi = raw[index + 1];
                let lo = raw[index + 2];
                let decode = |byte: u8| match byte {
                    b'0'..=b'9' => Some(byte - b'0'),
                    b'a'..=b'f' => Some(byte - b'a' + 10),
                    b'A'..=b'F' => Some(byte - b'A' + 10),
                    _ => None,
                };
                if let (Some(hi), Some(lo)) = (decode(hi), decode(lo)) {
                    bytes.push((hi << 4) | lo);
                    index += 3;
                } else {
                    bytes.push(raw[index]);
                    index += 1;
                }
            }
            byte => {
                bytes.push(byte);
                index += 1;
            }
        }
    }
    String::from_utf8_lossy(&bytes).into_owned()
}

fn shell_quote(value: &str) -> String {
    if value.is_empty() {
        return "\"\"".to_string();
    }
    if value
        .chars()
        .all(|ch| ch.is_ascii_alphanumeric() || matches!(ch, '/' | '_' | '-' | '.' | ':'))
    {
        value.to_string()
    } else {
        format!("\"{}\"", value.replace('\\', "\\\\").replace('"', "\\\""))
    }
}

fn render_page(
    runtime: &HostRuntime,
    flash_message: Option<&str>,
    flash_error: Option<&HostRuntimeError>,
) -> String {
    let snapshot = runtime.snapshot();
    let active_view = resolved_active_view(&snapshot);
    let selected_file = snapshot
        .selection
        .selected_paths
        .first()
        .cloned()
        .unwrap_or_default();
    let selected_commit = snapshot
        .selection
        .selected_commit_oid
        .clone()
        .unwrap_or_default();
    let selected_branch = snapshot
        .selection
        .selected_branch
        .clone()
        .unwrap_or_default();
    let selected_plugin = snapshot
        .selection
        .selected_plugin_id
        .clone()
        .unwrap_or_default();
    let repo_root = snapshot
        .repo
        .as_ref()
        .map(|repo| repo.root.as_str())
        .unwrap_or("");
    let compare_base = snapshot
        .compare
        .base_ref
        .as_deref()
        .or(snapshot.repo.as_ref().and_then(|repo| repo.head.as_deref()))
        .unwrap_or("");
    let compare_head = snapshot
        .compare
        .head_ref
        .as_deref()
        .or(snapshot.selection.selected_branch.as_deref())
        .unwrap_or("");
    let context_widgets =
        render_context_widgets(&snapshot, &active_view, compare_base, compare_head);
    let repo_state = if snapshot.repo.is_some() {
        "repo open"
    } else {
        "no repo"
    };

    format!(
        "<!doctype html><html lang=\"en\"><head><meta charset=\"utf-8\"><meta name=\"viewport\" content=\"width=device-width, initial-scale=1\"><title>Branchforge GUI</title><style>{}</style></head><body><div class=\"shell\" id=\"shell\" data-version=\"{}\"><header class=\"masthead\" id=\"masthead\">{}</header><section id=\"flash\">{}</section><main class=\"workspace\"><aside class=\"sidebar\" id=\"sidebar\">{}</aside><section class=\"content\" id=\"content\">{}{}{}</section></main></div><script>{}</script></body></html>",
        styles(),
        snapshot.version,
        render_masthead(&snapshot, repo_root, repo_state),
        render_flash(flash_message, flash_error),
        render_sidebar(&snapshot, &active_view, compare_base, compare_head),
        render_workspace_summary(
            &snapshot,
            &active_view,
            &selected_file,
            &selected_commit,
            &selected_branch,
            &selected_plugin,
        ),
        context_widgets,
        render_active_panel(runtime, &snapshot, &active_view),
        client_script(),
    )
}

fn render_masthead(snapshot: &StoreSnapshot, repo_root: &str, repo_state: &str) -> String {
    format!(
        "<div><p class=\"eyebrow\">Branchforge</p><h1>Runtime GUI</h1><p class=\"sub\">Focused controls over the same host runtime, jobs, state store, and action catalog.</p><div class=\"hero-metrics\"><span class=\"badge\">state v{}</span><span class=\"badge\">{}</span>{}</div></div><div class=\"masthead-tools\">{}{} </div>",
        snapshot.version,
        repo_state,
        snapshot
            .repo
            .as_ref()
            .and_then(|repo| repo.head.as_ref())
            .map(|head| format!("<span class=\"badge\">head {}</span>", escape_html(head)))
            .unwrap_or_default(),
        render_open_form(repo_root),
        render_command_bar(),
    )
}

fn render_open_form(repo_root: &str) -> String {
    format!(
        "<form method=\"post\" action=\"/command\" class=\"card tight open-form\"><label>Repository</label><input type=\"hidden\" name=\"gui_action\" value=\"repo_open\"><div class=\"row row-stretch\"><input type=\"text\" name=\"repo_path\" value=\"{}\" placeholder=\"/path/to/repo\"><button type=\"submit\">Open</button></div></form>",
        escape_html(repo_root)
    )
}

fn render_command_bar() -> String {
    "<form method=\"post\" action=\"/command\" class=\"card tight command-bar\"><label>Command Bar</label><div class=\"row row-stretch\"><input type=\"text\" name=\"command\" placeholder=\"run diagnostics.repo_capabilities\"><button type=\"submit\">Run</button></div><p class=\"meta\">Accepts the same runtime commands as the console runner.</p></form>".to_string()
}

fn render_sidebar(
    snapshot: &StoreSnapshot,
    active_view: &str,
    compare_base: &str,
    compare_head: &str,
) -> String {
    [
        render_fold_card(
            "Panels",
            render_panel_tabs(active_view, snapshot.repo.is_some()),
            true,
        ),
        render_fold_card("Quick Actions", render_quick_actions(snapshot), true),
        render_fold_card(
            "Primary Flows",
            render_workflow_widgets(snapshot, compare_base, compare_head),
            snapshot.repo.is_none(),
        ),
        render_fold_card("Selections", render_selection_summary(snapshot), true),
    ]
    .join("")
}

fn render_fold_card(title: &str, body: String, open: bool) -> String {
    format!(
        "<details class=\"card fold\"{}><summary><h2>{}</h2><span class=\"fold-icon\" aria-hidden=\"true\"></span></summary><div class=\"fold-body\">{}</div></details>",
        if open { " open" } else { "" },
        escape_html(title),
        body
    )
}

fn render_selection_summary(snapshot: &StoreSnapshot) -> String {
    let mut chips = Vec::new();
    if let Some(repo) = snapshot.repo.as_ref() {
        chips.push(format!(
            "<span class=\"badge badge-strong\">repo {}</span>",
            escape_html(&repo.root)
        ));
    }
    if let Some(head) = snapshot.repo.as_ref().and_then(|repo| repo.head.as_ref()) {
        chips.push(format!(
            "<span class=\"badge\">head {}</span>",
            escape_html(head)
        ));
    }
    if let Some(path) = snapshot.selection.selected_paths.first() {
        chips.push(format!(
            "<span class=\"badge\">file {}</span>",
            escape_html(path)
        ));
    }
    if let Some(commit) = snapshot.selection.selected_commit_oid.as_ref() {
        chips.push(format!(
            "<span class=\"badge\">commit {}</span>",
            escape_html(&short_oid(commit))
        ));
    }
    if let Some(branch) = snapshot.selection.selected_branch.as_ref() {
        chips.push(format!(
            "<span class=\"badge\">branch {}</span>",
            escape_html(branch)
        ));
    }
    if let Some(plugin) = snapshot.selection.selected_plugin_id.as_ref() {
        chips.push(format!(
            "<span class=\"badge\">plugin {}</span>",
            escape_html(plugin)
        ));
    }
    if chips.is_empty() {
        "<p class=\"meta\">No active selections yet. Pick a file, commit, branch, or plugin to unlock faster defaults.</p>".to_string()
    } else {
        format!("<div class=\"chip-list\">{}</div>", chips.join(""))
    }
}

fn render_workspace_summary(
    snapshot: &StoreSnapshot,
    active_view: &str,
    selected_file: &str,
    selected_commit: &str,
    selected_branch: &str,
    selected_plugin: &str,
) -> String {
    let mut chips = vec![format!(
        "<span class=\"badge badge-strong\">panel {}</span>",
        escape_html(active_view_title(active_view))
    )];
    if let Some(repo) = snapshot.repo.as_ref() {
        chips.push(format!(
            "<span class=\"badge\">repo {}</span>",
            escape_html(&repo.root)
        ));
    }
    if let Some(head) = snapshot.repo.as_ref().and_then(|repo| repo.head.as_ref()) {
        chips.push(format!(
            "<span class=\"badge\">head {}</span>",
            escape_html(head)
        ));
    }
    if !selected_file.is_empty() {
        chips.push(format!(
            "<span class=\"badge\">file {}</span>",
            escape_html(selected_file)
        ));
    }
    if !selected_commit.is_empty() {
        chips.push(format!(
            "<span class=\"badge\">commit {}</span>",
            escape_html(&short_oid(selected_commit))
        ));
    }
    if !selected_branch.is_empty() {
        chips.push(format!(
            "<span class=\"badge\">branch {}</span>",
            escape_html(selected_branch)
        ));
    }
    if !selected_plugin.is_empty() {
        chips.push(format!(
            "<span class=\"badge\">plugin {}</span>",
            escape_html(selected_plugin)
        ));
    }

    let quick_buttons = if snapshot.repo.is_some() {
        [
            render_command_button("refresh", "Refresh", "ghost"),
            render_command_button("panel diagnostics", "Diagnostics", "ghost"),
            render_command_button("panel logs", "Logs", "ghost"),
        ]
        .join("")
    } else {
        [render_command_button("panel logs", "Logs", "ghost")].join("")
    };

    format!(
        "<section class=\"card tight workspace-summary\"><div class=\"summary-head\"><div><p class=\"eyebrow eyebrow-small\">Workspace</p><h2>{}</h2></div><div class=\"inline-actions\">{}</div></div><div class=\"chip-list\">{}</div></section>",
        escape_html(active_view_title(active_view)),
        quick_buttons,
        chips.join("")
    )
}

fn active_view_title(active_view: &str) -> &'static str {
    match active_view {
        "empty.state" => "Getting Started",
        "status.panel" => "Status",
        "history.panel" => "History",
        "branches.panel" => "Branches",
        "tags.panel" => "Tags",
        "compare.panel" => "Compare",
        "diagnostics.panel" => "Diagnostics",
        "logs.panel" => "Logs",
        _ => "Workspace",
    }
}

fn resolved_active_view(snapshot: &StoreSnapshot) -> String {
    snapshot.active_view.clone().unwrap_or_else(|| {
        if snapshot.repo.is_some() {
            "status.panel".to_string()
        } else {
            "empty.state".to_string()
        }
    })
}

fn styles() -> &'static str {
    r#"
:root{
  --bg:#f3f6f9;
  --surface:#ffffff;
  --surface-muted:#f8fafc;
  --ink:#18212b;
  --muted:#667085;
  --line:#d9e1ea;
  --accent:#2563eb;
  --accent-soft:#eef4ff;
  --accent-strong:#1d4ed8;
  --warn:#b42318;
}
*{box-sizing:border-box}
body{
  margin:0;
  font-family:"SF Pro Text","Segoe UI",sans-serif;
  color:var(--ink);
  background:var(--bg);
}
.shell{max-width:1360px;margin:0 auto;padding:24px}
.masthead{
  display:grid;
  grid-template-columns:minmax(0,1fr) minmax(280px,420px);
  gap:12px;
  align-items:start;
  margin-bottom:12px;
}
.masthead-tools{display:grid;gap:10px}
.eyebrow{
  margin:0 0 6px;
  text-transform:uppercase;
  letter-spacing:.12em;
  font-size:.75rem;
  color:var(--accent);
}
.eyebrow-small{font-size:.68rem;margin-bottom:4px}
h1,h2,h3{
  margin:0;
  font-family:inherit;
  font-weight:600;
}
h1{font-size:clamp(1.8rem,4vw,2.5rem);line-height:1.05}
h2{font-size:.98rem;margin-bottom:8px}
.sub{max-width:46rem;color:var(--muted);font-size:.96rem;line-height:1.45}
.workspace{
  display:grid;
  grid-template-columns:280px minmax(0,1fr);
  gap:12px;
}
.sidebar,.content{display:grid;gap:10px;align-content:start}
.sidebar{position:sticky;top:12px;height:max-content}
.card{
  background:var(--surface);
  border:1px solid var(--line);
  border-radius:14px;
  padding:12px;
}
.card.tight{padding:10px 12px}
.fold summary{
  display:flex;
  justify-content:space-between;
  align-items:center;
  gap:10px;
  cursor:pointer;
  list-style:none;
}
.fold summary::-webkit-details-marker{display:none}
.fold-icon::before{content:"+";font-weight:700;color:var(--muted)}
.fold[open] .fold-icon::before{content:"−"}
.fold-body{display:grid;gap:10px;margin-top:10px}
.workspace-summary .summary-head{
  display:flex;
  justify-content:space-between;
  gap:10px;
  align-items:flex-start;
}
.flash{
  border-radius:12px;
  min-height:16px;
  padding:10px 12px;
  margin-bottom:12px;
  border:1px solid var(--line);
}
.flash.info{background:var(--accent-soft);color:var(--accent-strong)}
.flash.error{background:#fff1ee;color:var(--warn)}
.stack,.open-form{display:grid;gap:10px}
.command-bar,.open-form{gap:8px}
.row{display:flex;gap:8px;align-items:center;flex-wrap:wrap}
.row-stretch > *{flex:1}
.row-stretch button{flex:0 0 auto}
input[type=text],textarea,select{
  width:100%;
  border:1px solid var(--line);
  border-radius:10px;
  padding:9px 11px;
  background:var(--surface);
  color:var(--ink);
}
textarea{min-height:104px;resize:vertical}
button{
  border:1px solid transparent;
  border-radius:10px;
  padding:8px 11px;
  background:var(--accent);
  color:white;
  cursor:pointer;
  font-weight:600;
  font-size:.88rem;
}
button.secondary{
  background:var(--surface-muted);
  border-color:var(--line);
  color:var(--ink);
}
button.ghost{
  background:var(--accent-soft);
  border-color:#cfe0ff;
  color:var(--accent-strong);
}
button:disabled{
  cursor:not-allowed;
  opacity:.55;
}
.disabled-action{display:grid;gap:6px}
.tabs,.inline-actions,.list-actions,.chip-list{display:flex;gap:8px;flex-wrap:wrap}
.facts{display:grid;gap:10px;margin:0}
.facts div{display:grid;gap:4px}
dt{font-size:.78rem;text-transform:uppercase;letter-spacing:.08em;color:var(--muted)}
dd{margin:0;font-weight:600}
ul.clean{list-style:none;margin:0;padding:0;display:grid;gap:8px}
li.item{
  padding:10px;
  border:1px solid var(--line);
  border-radius:12px;
  background:var(--surface-muted);
}
.meta{color:var(--muted);font-size:.92rem}
pre{
  margin:0;
  white-space:pre-wrap;
  word-break:break-word;
  font-family:"IBM Plex Mono","SFMono-Regular",monospace;
  font-size:.9rem;
  line-height:1.45;
  max-height:28rem;
  overflow:auto;
}
.grid-two{display:grid;grid-template-columns:repeat(2,minmax(0,1fr));gap:12px}
.badge{
  display:inline-flex;
  align-items:center;
  border-radius:999px;
  padding:4px 9px;
  border:1px solid var(--line);
  background:var(--surface-muted);
  color:var(--muted);
  font-size:.82rem;
  font-weight:600;
}
.badge-strong{
  background:var(--accent-soft);
  border-color:#cfe0ff;
  color:var(--accent-strong);
}
.hero-metrics{display:flex;gap:8px;flex-wrap:wrap;margin-top:10px}
.widget-grid{display:grid;gap:10px}
.widget-grid form{display:grid;gap:8px}
.flow-grid{
  display:grid;
  grid-template-columns:repeat(2,minmax(0,1fr));
  gap:8px;
}
.mini-form{
  display:grid;
  gap:8px;
  padding:10px;
  border:1px solid var(--line);
  border-radius:12px;
  background:var(--surface-muted);
}
.checkbox{display:flex;gap:10px;align-items:center;color:var(--muted)}
.checkbox input{width:auto;margin:0}
.hunk-list{display:grid;gap:12px;margin-top:16px}
.hunk-card{
  border:1px solid var(--line);
  border-radius:12px;
  background:var(--surface-muted);
  padding:14px;
}
.hunk-meta{margin-bottom:10px}
.line-guide{max-height:20rem;overflow:auto}
@media (max-width: 1080px){
  .masthead,.workspace,.grid-two{grid-template-columns:1fr}
  .flow-grid{grid-template-columns:1fr}
  .sidebar{position:static}
  .shell{padding:16px}
}
"#
}

fn client_script() -> &'static str {
    r#"
const parseAndSwap = async (responseText, preserveFlash = false) => {
  const doc = new DOMParser().parseFromString(responseText, "text/html");
  const nextShell = doc.getElementById("shell");
  const currentShell = document.getElementById("shell");
  if (nextShell && currentShell && nextShell.dataset.version) {
    currentShell.dataset.version = nextShell.dataset.version;
  }
  for (const id of ["masthead", "sidebar", "content"]) {
    const next = doc.getElementById(id);
    const current = document.getElementById(id);
    if (next && current) current.innerHTML = next.innerHTML;
  }
  if (!preserveFlash) {
    const nextFlash = doc.getElementById("flash");
    const currentFlash = document.getElementById("flash");
    if (nextFlash && currentFlash) currentFlash.innerHTML = nextFlash.innerHTML;
  }
};

document.addEventListener("submit", async (event) => {
  const form = event.target.closest("form");
  if (!form || form.dataset.native === "true") return;
  event.preventDefault();
  const body = new URLSearchParams(new FormData(form, event.submitter));
  const response = await fetch(form.action || "/command", {
    method: (form.method || "post").toUpperCase(),
    headers: {
      "Content-Type": "application/x-www-form-urlencoded;charset=UTF-8",
      "X-Requested-With": "fetch"
    },
    body
  });
  const text = await response.text();
  await parseAndSwap(text, false);
});

window.setInterval(async () => {
  try {
    const response = await fetch("/", { headers: { "X-Requested-With": "fetch" } });
    const text = await response.text();
    const doc = new DOMParser().parseFromString(text, "text/html");
    const nextShell = doc.getElementById("shell");
    const currentShell = document.getElementById("shell");
    if (nextShell && currentShell && nextShell.dataset.version === currentShell.dataset.version) {
      return;
    }
    await parseAndSwap(text, true);
    if (nextShell && currentShell && nextShell.dataset.version) {
      currentShell.dataset.version = nextShell.dataset.version;
    }
  } catch (_) {
  }
}, 3000);
"#
}

fn render_flash(message: Option<&str>, error: Option<&HostRuntimeError>) -> String {
    if let Some(error) = error {
        return format!(
            "<div class=\"flash error\"><strong>{}</strong><div>{}</div>{}</div>",
            escape_html(&error.title),
            escape_html(&error.message),
            error
                .detail
                .as_deref()
                .map(|detail| format!("<pre>{}</pre>", escape_html(detail)))
                .unwrap_or_default()
        );
    }
    message
        .map(|message| {
            format!(
                "<div class=\"flash info\"><strong>Command completed</strong><div>{}</div></div>",
                escape_html(message)
            )
        })
        .unwrap_or_default()
}

fn render_panel_tabs(active_view: &str, repo_open: bool) -> String {
    let mut panels = vec![("Logs", "panel logs", "logs.panel")];
    if repo_open {
        panels.splice(
            0..0,
            [
                ("Status", "panel status", "status.panel"),
                ("History", "panel history", "history.panel"),
                ("Branches", "panel branches", "branches.panel"),
                ("Tags", "panel tags", "tags.panel"),
                ("Compare", "panel compare", "compare.panel"),
                ("Diagnostics", "panel diagnostics", "diagnostics.panel"),
            ],
        );
    }

    let tabs = panels
        .into_iter()
        .map(|(label, command, view)| {
            format!(
                "<form method=\"post\" action=\"/command\"><input type=\"hidden\" name=\"command\" value=\"{}\"><button class=\"{}\" type=\"submit\">{}</button></form>",
                escape_html(command),
                if view == active_view { "" } else { "secondary" },
                label
            )
        })
        .collect::<Vec<_>>()
        .join("");
    if repo_open {
        format!("<div class=\"tabs\">{tabs}</div>")
    } else {
        format!(
            "<p class=\"meta\">Open a repository to unlock runtime panels.</p><div class=\"tabs\">{tabs}</div>"
        )
    }
}

fn render_quick_actions(snapshot: &StoreSnapshot) -> String {
    if snapshot.repo.is_none() {
        let items = [
            render_command_button("panel logs", "Logs", "ghost"),
            render_command_button("run plugin.list", "Plugins", "ghost"),
            render_command_button("ops", "Ops", "ghost"),
        ]
        .join("");
        return format!(
            "<p class=\"meta\">Open a repository to unlock repo-backed actions.</p><div class=\"inline-actions\">{items}</div>"
        );
    }

    let mut buttons = vec![
        ("Refresh", "refresh"),
        ("Status", "run status.refresh"),
        ("Refs", "run refs.refresh"),
        ("History", "run history.page 0 20"),
        ("Capabilities", "run diagnostics.repo_capabilities"),
        ("Logs", "panel logs"),
        ("Plugins", "plugin list"),
        ("Ops", "ops"),
    ];
    if !snapshot.selection.selected_paths.is_empty() {
        buttons.push(("Worktree Diff", "run diff.worktree"));
        buttons.push(("Stage Selected", "run index.stage_selected"));
        buttons.push(("Unstage Selected", "run index.unstage_selected"));
        buttons.push(("Blame File", "run blame.file"));
    }
    if snapshot.selection.selected_commit_oid.is_some() {
        buttons.push(("Commit Diff", "run diff.commit"));
        buttons.push(("Commit Details", "run history.details"));
    }
    if snapshot.selection.selected_branch.is_some() {
        buttons.push(("Compare", "run compare.refs"));
        buttons.push(("Checkout", "run branch.checkout"));
    }

    let items = buttons
        .into_iter()
        .map(|(label, command)| {
            format!(
                "<form method=\"post\" action=\"/command\"><input type=\"hidden\" name=\"command\" value=\"{}\"><button class=\"ghost\" type=\"submit\">{}</button></form>",
                escape_html(command),
                label
            )
        })
        .collect::<Vec<_>>()
        .join("");
    format!("<div class=\"inline-actions\">{items}</div>")
}

fn render_workflow_widgets(
    snapshot: &StoreSnapshot,
    compare_base: &str,
    compare_head: &str,
) -> String {
    if snapshot.repo.is_none() {
        return "<p class=\"meta\">Repository workflows appear here after you open a Git repository.</p>"
            .to_string();
    }

    let branch_base = snapshot
        .repo
        .as_ref()
        .and_then(|repo| repo.head.as_deref())
        .unwrap_or("HEAD");
    let selected_branch = snapshot.selection.selected_branch.as_deref().unwrap_or("");
    let selected_file = snapshot
        .selection
        .selected_paths
        .first()
        .map(String::as_str)
        .unwrap_or("");
    format!(
        "<div class=\"flow-grid\">\
            <form method=\"post\" action=\"/command\" class=\"mini-form\">\
                <input type=\"hidden\" name=\"gui_action\" value=\"commit_create\">\
                <label>Commit</label>\
                <input type=\"text\" name=\"commit_message\" placeholder=\"Describe the staged change\">\
                <div class=\"inline-actions\"><button type=\"submit\">Create Commit</button></div>\
            </form>\
            <form method=\"post\" action=\"/command\" class=\"mini-form\">\
                <input type=\"hidden\" name=\"gui_action\" value=\"branch_create\">\
                <label>New Branch</label>\
                <input type=\"text\" name=\"branch_name\" placeholder=\"feature/gui-shell\">\
                <input type=\"text\" name=\"branch_base\" value=\"{}\">\
                <div class=\"inline-actions\"><button type=\"submit\">Create Branch</button></div>\
            </form>\
            <form method=\"post\" action=\"/command\" class=\"mini-form\">\
                <input type=\"hidden\" name=\"gui_action\" value=\"branch_checkout\">\
                <label>Checkout</label>\
                <input type=\"text\" name=\"branch_name\" value=\"{}\" placeholder=\"selected branch or explicit ref\">\
                <div class=\"inline-actions\"><button type=\"submit\">Checkout Branch</button></div>\
            </form>\
            <form method=\"post\" action=\"/command\" class=\"mini-form\">\
                <input type=\"hidden\" name=\"gui_action\" value=\"compare_refs\">\
                <label>Compare</label>\
                <input type=\"text\" name=\"compare_base\" value=\"{}\">\
                <input type=\"text\" name=\"compare_head\" value=\"{}\">\
                <div class=\"inline-actions\"><button type=\"submit\">Compare Refs</button></div>\
            </form>\
            <form method=\"post\" action=\"/command\" class=\"mini-form\">\
                <input type=\"hidden\" name=\"gui_action\" value=\"history_file\">\
                <label>File History</label>\
                <input type=\"text\" name=\"history_file_path\" value=\"{}\" placeholder=\"selected file or explicit path\">\
                <div class=\"inline-actions\">\
                    <button class=\"secondary\" type=\"submit\">File History</button>\
                </div>\
            </form>\
            <form method=\"post\" action=\"/command\" class=\"mini-form\">\
                <input type=\"hidden\" name=\"gui_action\" value=\"blame_file\">\
                <label>Blame File</label>\
                <input type=\"text\" name=\"history_file_path\" value=\"{}\" placeholder=\"selected file or explicit path\">\
                <div class=\"inline-actions\"><button type=\"submit\">Load Blame</button></div>\
            </form>\
        </div>",
        escape_html(branch_base),
        escape_html(selected_branch),
        escape_html(compare_base),
        escape_html(compare_head),
        escape_html(selected_file),
        escape_html(selected_file)
    )
}

fn render_context_widgets(
    snapshot: &StoreSnapshot,
    active_view: &str,
    compare_base: &str,
    compare_head: &str,
) -> String {
    let sections = match active_view {
        "empty.state" => vec![render_empty_state_controls()],
        "history.panel" => vec![
            render_history_controls(snapshot),
            render_history_commit_controls(snapshot),
        ],
        "branches.panel" => vec![
            render_branch_controls(snapshot),
            render_advanced_branch_controls(snapshot),
            render_rebase_controls(snapshot),
            render_conflict_controls(snapshot),
        ],
        "tags.panel" => vec![render_tag_controls(snapshot)],
        "compare.panel" => vec![render_compare_controls(compare_base, compare_head)],
        "diagnostics.panel" => vec![
            render_diagnostics_controls(snapshot),
            render_plugin_controls(snapshot),
            render_runtime_ops_controls(),
        ],
        "logs.panel" => vec![render_logs_controls(snapshot)],
        _ => vec![
            render_status_controls(snapshot),
            render_stash_worktree_controls(),
            render_diff_controls(snapshot),
        ],
    };
    format!(
        "<section class=\"grid-two\">{}</section>",
        sections.join("")
    )
}

fn render_empty_state_controls() -> String {
    format!(
        "<section class=\"card\"><h2>Open A Repository</h2><p class=\"meta\">Use the repository form above to attach the GUI to a Git working tree. Once a repository is open, Branchforge will unlock status, history, branches, tags, compare, diagnostics, diff controls, and the full runtime workspace.</p><div class=\"inline-actions\">{}{}{}</div></section>",
        render_command_button("panel logs", "Logs", "ghost"),
        render_command_button("run plugin.list", "Plugins", "ghost"),
        render_command_button("ops", "Ops", "ghost")
    )
}

fn render_logs_controls(snapshot: &StoreSnapshot) -> String {
    let entries = snapshot.journal.entries.len();
    let refresh_button = if snapshot.repo.is_some() {
        render_command_button("refresh", "Refresh Repo", "ghost")
    } else {
        String::new()
    };
    format!(
        "<section class=\"card\"><h2>System Tools</h2><p class=\"meta\">Recent journal entries: {}. This page keeps runtime state, operation history, actions, and ops catalog in one place instead of repeating them across every panel.</p><div class=\"inline-actions\">{}{}{}{}{}</div></section>",
        entries,
        refresh_button,
        render_command_button(
            "run diagnostics.journal_summary",
            "Journal Summary",
            "ghost"
        ),
        render_command_button("panel diagnostics", "Diagnostics", "ghost"),
        render_command_button("run plugin.list", "Plugin Inventory", "ghost"),
        render_command_button("ops", "Ops Catalog", "ghost")
    )
}

fn render_status_controls(snapshot: &StoreSnapshot) -> String {
    let selected_file = snapshot
        .selection
        .selected_paths
        .first()
        .map(String::as_str)
        .unwrap_or("");
    let actions = [
        render_command_button("refresh", "Refresh Repo", "ghost"),
        render_command_button("run status.refresh", "Status Snapshot", "ghost"),
        render_command_button("run refs.refresh", "Refs Snapshot", "ghost"),
        render_command_button("run diff.worktree", "Diff Worktree", "ghost"),
        render_command_button("run diff.index", "Diff Index", "ghost"),
        render_command_button("run index.stage_selected", "Stage Selected", "ghost"),
        render_command_button("run index.unstage_selected", "Unstage Selected", "ghost"),
        render_command_button("run --confirm file.discard", "Discard Selected", "ghost"),
    ]
    .join("");
    format!(
        "<section class=\"card\"><h2>Workspace Controls</h2><div class=\"inline-actions\">{}</div><p class=\"meta\">Selected file: {}</p></section>",
        actions,
        display_value(selected_file)
    )
}

fn render_stash_worktree_controls() -> String {
    format!(
        "<section class=\"card\"><h2>Stash / Worktree</h2>\
            <div class=\"widget-grid\">\
                <form method=\"post\" action=\"/command\">\
                    <input type=\"hidden\" name=\"gui_action\" value=\"stash_create\">\
                    <label>Stash Message</label>\
                    <input type=\"text\" name=\"stash_message\" placeholder=\"wip/gui-flow\">\
                    <button type=\"submit\">Create Stash</button>\
                </form>\
                <form method=\"post\" action=\"/command\">\
                    <input type=\"hidden\" name=\"gui_action\" value=\"stash_apply\">\
                    <label>Stash Selector</label>\
                    <input type=\"text\" name=\"stash_selector\" placeholder=\"stash@{{0}} or 0\">\
                    <div class=\"inline-actions\">\
                        <button type=\"submit\">Apply</button>\
                    </div>\
                </form>\
                <form method=\"post\" action=\"/command\">\
                    <input type=\"hidden\" name=\"gui_action\" value=\"stash_pop\">\
                    <label>Stash Selector</label>\
                    <input type=\"text\" name=\"stash_selector\" placeholder=\"stash@{{0}} or 0\">\
                    <div class=\"inline-actions\">\
                        <button type=\"submit\">Pop</button>\
                    </div>\
                </form>\
                <form method=\"post\" action=\"/command\">\
                    <input type=\"hidden\" name=\"gui_action\" value=\"stash_drop\">\
                    <label>Stash Selector</label>\
                    <input type=\"text\" name=\"stash_selector\" placeholder=\"stash@{{0}} or 0\">\
                    <div class=\"inline-actions\">\
                        <button class=\"secondary\" type=\"submit\">Drop</button>\
                    </div>\
                </form>\
                <div class=\"inline-actions\">{}\
                </div>\
                <form method=\"post\" action=\"/command\">\
                    <input type=\"hidden\" name=\"gui_action\" value=\"worktree_create\">\
                    <label>Worktree Path</label>\
                    <input type=\"text\" name=\"worktree_path\" placeholder=\"../branchforge-feature\">\
                    <label>Branch</label>\
                    <input type=\"text\" name=\"worktree_branch\" placeholder=\"feature/gui-shell\">\
                    <button type=\"submit\">Create Worktree</button>\
                </form>\
                <form method=\"post\" action=\"/command\">\
                    <input type=\"hidden\" name=\"gui_action\" value=\"worktree_open\">\
                    <label>Worktree Path</label>\
                    <input type=\"text\" name=\"worktree_path\" placeholder=\"../branchforge-feature\">\
                    <div class=\"inline-actions\">\
                        <button type=\"submit\">Open Worktree</button>\
                    </div>\
                </form>\
                <form method=\"post\" action=\"/command\">\
                    <input type=\"hidden\" name=\"gui_action\" value=\"worktree_remove\">\
                    <label>Worktree Path</label>\
                    <input type=\"text\" name=\"worktree_path\" placeholder=\"../branchforge-feature\">\
                    <div class=\"inline-actions\">\
                        <button class=\"secondary\" type=\"submit\">Remove Worktree</button>\
                    </div>\
                </form>\
                <div class=\"inline-actions\">{}\
                </div>\
                <form method=\"post\" action=\"/command\">\
                    <input type=\"hidden\" name=\"gui_action\" value=\"submodule_open\">\
                    <label>Submodule Path</label>\
                    <input type=\"text\" name=\"submodule_path\" placeholder=\"vendor/lib\">\
                    <div class=\"inline-actions\">\
                        <button type=\"submit\">Open Submodule</button>\
                        <button type=\"submit\" name=\"gui_action\" value=\"submodule_init_update\" class=\"ghost\">Init / Update Path</button>\
                    </div>\
                </form>\
                <div class=\"inline-actions\">{}{}{}{}\
                </div>\
            </div>\
        </section>",
        render_command_button("run stash.list", "List", "ghost"),
        render_command_button("run worktree.list", "List Worktrees", "ghost"),
        render_command_button("run submodule.list", "List Submodules", "ghost"),
        render_command_button("run submodule.init_update", "Init/Update", "ghost"),
        render_command_button(
            "run diagnostics.repo_capabilities",
            "Repo Capabilities",
            "ghost"
        ),
        render_command_button("run diagnostics.lfs_status", "LFS Status", "ghost")
    )
}

fn render_diff_controls(snapshot: &StoreSnapshot) -> String {
    let selected_file = snapshot
        .selection
        .selected_paths
        .first()
        .map(String::as_str)
        .unwrap_or("");
    format!(
        "<section class=\"card\"><h2>File Navigation</h2>\
            <div class=\"widget-grid\">\
                <form method=\"post\" action=\"/command\">\
                    <input type=\"hidden\" name=\"gui_action\" value=\"history_file\">\
                    <label>Path</label>\
                    <input type=\"text\" name=\"history_file_path\" value=\"{}\" placeholder=\"selected file or explicit path\">\
                    <label>Limit</label>\
                    <input type=\"text\" name=\"history_limit\" value=\"20\">\
                    <div class=\"inline-actions\">\
                        <button type=\"submit\">File History</button>\
                    </div>\
                </form>\
                <div class=\"inline-actions\">{}{}\
                </div>\
            </div>\
        </section>",
        escape_html(selected_file),
        render_command_button("run diff.worktree", "Worktree Diff", "ghost"),
        render_command_button("run diff.index", "Index Diff", "ghost")
    )
}

fn render_history_controls(snapshot: &StoreSnapshot) -> String {
    let selected_file = snapshot
        .selection
        .selected_paths
        .first()
        .map(String::as_str)
        .unwrap_or("");
    let selected_commit = snapshot
        .selection
        .selected_commit_oid
        .as_deref()
        .unwrap_or("");
    format!(
        "<section class=\"card\"><h2>History Queries</h2>\
            <div class=\"widget-grid\">\
                <form method=\"post\" action=\"/command\">\
                    <input type=\"hidden\" name=\"gui_action\" value=\"history_filter\">\
                    <label>Author</label>\
                    <input type=\"text\" name=\"history_author\" value=\"{}\" placeholder=\"optional author filter\">\
                    <label>Text</label>\
                    <input type=\"text\" name=\"history_text\" value=\"{}\" placeholder=\"summary or message text\">\
                    <label>Hash Prefix</label>\
                    <input type=\"text\" name=\"history_hash_prefix\" placeholder=\"optional short hash\">\
                    <label>Limit</label>\
                    <input type=\"text\" name=\"history_limit\" value=\"{}\">\
                    <div class=\"inline-actions\">\
                        <button type=\"submit\">Load History</button>\
                        <button type=\"submit\" name=\"gui_action\" value=\"history_search\" class=\"ghost\">Search Op</button>\
                    </div>\
                </form>\
                <div class=\"inline-actions\">{}{}\
                </div>\
                <form method=\"post\" action=\"/command\">\
                    <input type=\"hidden\" name=\"gui_action\" value=\"history_file\">\
                    <label>File Path</label>\
                    <input type=\"text\" name=\"history_file_path\" value=\"{}\" placeholder=\"selected file or explicit path\">\
                    <label>Limit</label>\
                    <input type=\"text\" name=\"history_limit\" value=\"20\">\
                    <div class=\"inline-actions\">\
                        <button type=\"submit\">File History</button>\
                        <button type=\"submit\" name=\"gui_action\" value=\"blame_file\" class=\"secondary\">Blame File</button>\
                    </div>\
                </form>\
                <div class=\"inline-actions\">{}{}{}\
                </div>\
                <p class=\"meta\">Selected commit: {}</p>\
            </div>\
        </section>",
        escape_html(snapshot.history.filter_author.as_deref().unwrap_or("")),
        escape_html(snapshot.history.filter_text.as_deref().unwrap_or("")),
        snapshot
            .history
            .next_cursor
            .as_ref()
            .map(|cursor| cursor.page_size)
            .unwrap_or(20),
        render_command_button("run history.clear_filter", "Clear Filter", "ghost"),
        if snapshot.history_can_load_more() {
            render_command_button("run history.load_more", "Load More", "ghost")
        } else {
            String::new()
        },
        escape_html(selected_file),
        render_command_button("run history.details", "Commit Details", "ghost"),
        render_command_button("run diff.commit", "Commit Diff", "ghost"),
        render_command_button("run blame.file", "Blame Selected", "ghost"),
        display_value(selected_commit)
    )
}

fn render_history_commit_controls(snapshot: &StoreSnapshot) -> String {
    let selected_commit = snapshot
        .selection
        .selected_commit_oid
        .as_deref()
        .unwrap_or("");
    format!(
        "<section class=\"card\"><h2>Commit Controls</h2>\
            <div class=\"widget-grid\">\
                <form method=\"post\" action=\"/command\">\
                    <input type=\"hidden\" name=\"gui_action\" value=\"commit_amend\">\
                    <label>Amend Message</label>\
                    <input type=\"text\" name=\"commit_message\" value=\"{}\" placeholder=\"rewrite staged commit message\">\
                    <button type=\"submit\">Amend Commit</button>\
                </form>\
                <form method=\"post\" action=\"/command\">\
                    <label>Commit Oid</label>\
                    <input type=\"text\" name=\"commit_oid\" value=\"{}\" placeholder=\"selected commit or explicit oid\">\
                    <div class=\"inline-actions\">\
                        <button type=\"submit\" name=\"gui_action\" value=\"cherry_pick_commit\">Cherry-pick</button>\
                        <button type=\"submit\" name=\"gui_action\" value=\"revert_commit\" class=\"ghost\">Revert</button>\
                        <button type=\"submit\" name=\"gui_action\" value=\"cherry_pick_abort\" class=\"secondary\">Abort Cherry-pick</button>\
                    </div>\
                </form>\
            </div>\
        </section>",
        escape_html(&snapshot.commit_message.draft),
        escape_html(selected_commit)
    )
}

fn render_branch_controls(snapshot: &StoreSnapshot) -> String {
    let selected_branch = snapshot.selection.selected_branch.as_deref().unwrap_or("");
    let branch_base = snapshot
        .repo
        .as_ref()
        .and_then(|repo| repo.head.as_deref())
        .unwrap_or("HEAD");
    format!(
        "<section class=\"card\"><h2>Branch Controls</h2>\
            <div class=\"widget-grid\">\
                <form method=\"post\" action=\"/command\">\
                    <input type=\"hidden\" name=\"gui_action\" value=\"branch_create\">\
                    <label>Branch Name</label>\
                    <input type=\"text\" name=\"branch_name\" placeholder=\"feature/branchforge-gui\">\
                    <label>Base Ref</label>\
                    <input type=\"text\" name=\"branch_base\" value=\"{}\">\
                    <button type=\"submit\">Create Branch</button>\
                </form>\
                <form method=\"post\" action=\"/command\">\
                    <input type=\"hidden\" name=\"gui_action\" value=\"branch_checkout\">\
                    <label>Checkout Branch</label>\
                    <input type=\"text\" name=\"branch_name\" value=\"{}\" placeholder=\"selected branch or explicit ref\">\
                    <button type=\"submit\">Checkout</button>\
                </form>\
                <form method=\"post\" action=\"/command\">\
                    <input type=\"hidden\" name=\"gui_action\" value=\"branch_rename\">\
                    <label>Current Name</label>\
                    <input type=\"text\" name=\"branch_old_name\" value=\"{}\" placeholder=\"optional; selected branch is used if empty\">\
                    <label>New Name</label>\
                    <input type=\"text\" name=\"branch_new_name\" placeholder=\"feature/gui-polish\">\
                    <button type=\"submit\">Rename Branch</button>\
                </form>\
                <form method=\"post\" action=\"/command\">\
                    <input type=\"hidden\" name=\"gui_action\" value=\"branch_delete\">\
                    <label>Delete Branch</label>\
                    <input type=\"text\" name=\"branch_name\" value=\"{}\" placeholder=\"branch name\">\
                    <button class=\"secondary\" type=\"submit\">Delete Branch</button>\
                </form>\
            </div>\
        </section>",
        escape_html(branch_base),
        escape_html(selected_branch),
        escape_html(selected_branch),
        escape_html(selected_branch)
    )
}

fn render_advanced_branch_controls(snapshot: &StoreSnapshot) -> String {
    let selected_branch = snapshot.selection.selected_branch.as_deref().unwrap_or("");
    format!(
        "<section class=\"card\"><h2>Advanced Branch Ops</h2>\
            <div class=\"widget-grid\">\
                <form method=\"post\" action=\"/command\">\
                    <input type=\"hidden\" name=\"gui_action\" value=\"merge_execute\">\
                    <label>Merge Source Ref</label>\
                    <input type=\"text\" name=\"merge_source_ref\" value=\"{}\" placeholder=\"selected branch or explicit ref\">\
                    <label>Merge Mode</label>\
                    <select name=\"merge_mode\">\
                        <option value=\"ff\">ff</option>\
                        <option value=\"fast-forward\">fast-forward</option>\
                        <option value=\"no-ff\">no-ff</option>\
                        <option value=\"squash\">squash</option>\
                    </select>\
                    <div class=\"inline-actions\">\
                        <button type=\"submit\">Merge</button>\
                        <button type=\"submit\" name=\"gui_action\" value=\"merge_abort\" class=\"secondary\">Abort Merge</button>\
                    </div>\
                </form>\
                <form method=\"post\" action=\"/command\">\
                    <input type=\"hidden\" name=\"gui_action\" value=\"reset_refs\">\
                    <label>Reset Mode</label>\
                    <select name=\"reset_mode\">\
                        <option value=\"soft\">soft</option>\
                        <option value=\"mixed\" selected>mixed</option>\
                        <option value=\"hard\">hard</option>\
                    </select>\
                    <label>Target</label>\
                    <input type=\"text\" name=\"reset_target\" placeholder=\"HEAD~1 or explicit ref\">\
                    <button class=\"secondary\" type=\"submit\">Reset Refs</button>\
                </form>\
            </div>\
        </section>",
        escape_html(selected_branch)
    )
}

fn render_rebase_controls(snapshot: &StoreSnapshot) -> String {
    let base_ref = snapshot
        .rebase
        .plan
        .as_ref()
        .map(|plan| plan.base_ref.as_str())
        .or(snapshot.repo.as_ref().and_then(|repo| repo.head.as_deref()))
        .unwrap_or("HEAD");
    format!(
        "<section class=\"card\"><h2>Rebase Controls</h2>\
            <div class=\"widget-grid\">\
                <form method=\"post\" action=\"/command\">\
                    <input type=\"hidden\" name=\"gui_action\" value=\"rebase_plan_create\">\
                    <label>Base Ref</label>\
                    <input type=\"text\" name=\"rebase_base_ref\" value=\"{}\">\
                    <div class=\"inline-actions\">\
                        <button type=\"submit\">Create Plan</button>\
                        <button type=\"submit\" name=\"gui_action\" value=\"rebase_interactive\" class=\"secondary\">Interactive</button>\
                    </div>\
                    <label class=\"checkbox\"><input type=\"checkbox\" name=\"rebase_autosquash\" value=\"autosquash\"{}> autosquash</label>\
                </form>\
                <form method=\"post\" action=\"/command\">\
                    <input type=\"hidden\" name=\"gui_action\" value=\"rebase_execute\">\
                    <label class=\"checkbox\"><input type=\"checkbox\" name=\"rebase_autosquash\" value=\"autosquash\"> autosquash on execute</label>\
                    <div class=\"inline-actions\">\
                        <button type=\"submit\">Execute Plan</button>\
                        <button type=\"submit\" name=\"gui_action\" value=\"rebase_continue\" class=\"ghost\">Continue</button>\
                        <button type=\"submit\" name=\"gui_action\" value=\"rebase_skip\" class=\"ghost\">Skip</button>\
                        <button type=\"submit\" name=\"gui_action\" value=\"rebase_abort\" class=\"secondary\">Abort</button>\
                        <button type=\"submit\" name=\"gui_action\" value=\"rebase_clear\" class=\"ghost\">Clear Plan</button>\
                    </div>\
                </form>\
            </div>\
        </section>",
        escape_html(base_ref),
        if snapshot
            .rebase
            .plan
            .as_ref()
            .is_some_and(|plan| plan.autosquash_aware)
        {
            " checked"
        } else {
            ""
        }
    )
}

fn render_conflict_controls(snapshot: &StoreSnapshot) -> String {
    let selected_file = snapshot
        .selection
        .selected_paths
        .first()
        .map(String::as_str)
        .unwrap_or("");
    format!(
        "<section class=\"card\"><h2>Conflict Controls</h2>\
            <div class=\"widget-grid\">\
                <div class=\"inline-actions\">{}{}{}\
                </div>\
                <form method=\"post\" action=\"/command\">\
                    <input type=\"hidden\" name=\"gui_action\" value=\"conflict_focus\">\
                    <label>Conflict Path</label>\
                    <input type=\"text\" name=\"conflict_path\" value=\"{}\" placeholder=\"selected file or explicit path\">\
                    <div class=\"inline-actions\">\
                        <button type=\"submit\">Focus</button>\
                        <button type=\"submit\" name=\"gui_action\" value=\"conflict_resolve_ours\" class=\"ghost\">Use Ours</button>\
                        <button type=\"submit\" name=\"gui_action\" value=\"conflict_resolve_theirs\" class=\"ghost\">Use Theirs</button>\
                        <button type=\"submit\" name=\"gui_action\" value=\"conflict_mark_resolved\" class=\"secondary\">Mark Resolved</button>\
                    </div>\
                </form>\
                <div class=\"inline-actions\">{}{}\
                </div>\
                <p class=\"meta\">Conflict state: {}</p>\
            </div>\
        </section>",
        render_command_button("run conflict.list", "List Conflicts", "ghost"),
        render_command_button("run conflict.continue", "Continue", "ghost"),
        render_command_button("run conflict.abort", "Abort", "secondary"),
        escape_html(selected_file),
        render_command_button("run conflict.focus", "Focus Selected", "ghost"),
        render_command_button("run refresh", "Refresh Repo", "ghost"),
        display_value(
            snapshot
                .repo
                .as_ref()
                .and_then(|repo| repo.conflict_state.as_ref())
                .map(|state| format!("{state:?}").to_lowercase())
                .as_deref()
                .unwrap_or("")
        )
    )
}

fn render_tag_controls(snapshot: &StoreSnapshot) -> String {
    let selected_tag = snapshot
        .tags
        .tags
        .first()
        .map(|tag| tag.name.as_str())
        .unwrap_or("");
    format!(
        "<section class=\"card\"><h2>Tag Controls</h2>\
            <div class=\"widget-grid\">\
                <form method=\"post\" action=\"/command\">\
                    <input type=\"hidden\" name=\"gui_action\" value=\"tag_create\">\
                    <label>Tag Name</label>\
                    <input type=\"text\" name=\"tag_name\" placeholder=\"v1.0.2\">\
                    <label>Target</label>\
                    <input type=\"text\" name=\"tag_target\" placeholder=\"HEAD\">\
                    <button type=\"submit\">Create Tag</button>\
                </form>\
                <form method=\"post\" action=\"/command\">\
                    <input type=\"hidden\" name=\"gui_action\" value=\"tag_checkout\">\
                    <label>Tag Name</label>\
                    <input type=\"text\" name=\"tag_name\" value=\"{}\" placeholder=\"tag name\">\
                    <div class=\"inline-actions\">\
                        <button type=\"submit\">Checkout Tag</button>\
                        <button type=\"submit\" name=\"gui_action\" value=\"tag_delete\" class=\"secondary\">Delete Tag</button>\
                    </div>\
                </form>\
            </div>\
        </section>",
        escape_html(selected_tag)
    )
}

fn render_compare_controls(compare_base: &str, compare_head: &str) -> String {
    format!(
        "<section class=\"card\"><h2>Compare Controls</h2>\
            <form method=\"post\" action=\"/command\" class=\"widget-grid\">\
                <input type=\"hidden\" name=\"gui_action\" value=\"compare_refs\">\
                <label>Base Ref</label>\
                <input type=\"text\" name=\"compare_base\" value=\"{}\">\
                <label>Head Ref</label>\
                <input type=\"text\" name=\"compare_head\" value=\"{}\">\
                <div class=\"inline-actions\">\
                    <button type=\"submit\">Load Compare</button>\
                </div>\
            </form>\
            <div class=\"inline-actions\">{}{}\
            </div>\
        </section>",
        escape_html(compare_base),
        escape_html(compare_head),
        render_command_button("run compare.refs", "Compare Selected Branch", "ghost"),
        render_command_button("run diff.commit", "Selected Commit Diff", "ghost")
    )
}

fn render_diagnostics_controls(snapshot: &StoreSnapshot) -> String {
    let lfs_controls = match snapshot.repo_capabilities.as_ref() {
        Some(caps) if !caps.lfs_available => format!(
            "{}{}{}<p class=\"meta\">Install git-lfs to enable LFS actions on this machine.</p>",
            render_disabled_button("LFS Status", "ghost"),
            render_disabled_button("LFS Fetch", "ghost"),
            render_disabled_button("LFS Pull", "ghost")
        ),
        _ => format!(
            "{}{}{}",
            render_command_button("run diagnostics.lfs_status", "LFS Status", "ghost"),
            render_command_button("run diagnostics.lfs_fetch", "LFS Fetch", "ghost"),
            render_command_button("run diagnostics.lfs_pull", "LFS Pull", "ghost")
        ),
    };
    format!(
        "<section class=\"card\"><h2>Diagnostics</h2>\
            <div class=\"widget-grid\">\
                <div class=\"inline-actions\">{}{}\
                </div>\
                <div class=\"inline-actions\">{}\
                </div>\
            </div>\
        </section>",
        render_command_button(
            "run diagnostics.repo_capabilities",
            "Repo Capabilities",
            "ghost"
        ),
        render_command_button(
            "run diagnostics.journal_summary",
            "Journal Summary",
            "ghost"
        ),
        lfs_controls
    )
}

fn render_plugin_controls(snapshot: &StoreSnapshot) -> String {
    let selected_plugin = snapshot
        .selection
        .selected_plugin_id
        .as_deref()
        .unwrap_or("");
    format!(
        "<section class=\"card\"><h2>Plugin Controls</h2>\
            <div class=\"widget-grid\">\
                <div class=\"inline-actions\">{}{}\
                </div>\
                <form method=\"post\" action=\"/command\">\
                    <input type=\"hidden\" name=\"gui_action\" value=\"plugin_discover\">\
                    <label>Registry Path</label>\
                    <input type=\"text\" name=\"registry_path\" placeholder=\"plugin_registry/registry.json\">\
                    <div class=\"inline-actions\">\
                        <button type=\"submit\">Discover Registry</button>\
                        <button type=\"submit\" name=\"gui_action\" value=\"plugin_install_registry\" class=\"secondary\">Install Registry Plugin</button>\
                    </div>\
                    <label>Plugin Id</label>\
                    <input type=\"text\" name=\"plugin_id\" value=\"{}\" placeholder=\"plugin id\">\
                </form>\
                <form method=\"post\" action=\"/command\">\
                    <input type=\"hidden\" name=\"gui_action\" value=\"plugin_install\">\
                    <label>Package Dir</label>\
                    <input type=\"text\" name=\"package_dir\" placeholder=\"external_plugins/sample_plugin\">\
                    <button type=\"submit\">Install Package Dir</button>\
                </form>\
                <form method=\"post\" action=\"/command\">\
                    <input type=\"hidden\" name=\"gui_action\" value=\"plugin_enable\">\
                    <label>Plugin Id</label>\
                    <input type=\"text\" name=\"plugin_id\" value=\"{}\" placeholder=\"selected plugin or explicit id\">\
                    <div class=\"inline-actions\">\
                        <button type=\"submit\">Enable</button>\
                        <button type=\"submit\" name=\"gui_action\" value=\"plugin_disable\" class=\"ghost\">Disable</button>\
                        <button type=\"submit\" name=\"gui_action\" value=\"plugin_remove\" class=\"secondary\">Remove</button>\
                    </div>\
                </form>\
            </div>\
        </section>",
        render_command_button("run plugin.list", "List Installed", "ghost"),
        render_command_button("plugin list", "Runtime Inventory", "ghost"),
        escape_html(selected_plugin),
        escape_html(selected_plugin)
    )
}

fn render_runtime_ops_controls() -> String {
    format!(
        "<section class=\"card\"><h2>Runtime Ops</h2>\
            <div class=\"widget-grid\">\
                <div class=\"inline-actions\">{}{}{}{}{}\
                </div>\
                <form method=\"post\" action=\"/command\">\
                    <input type=\"hidden\" name=\"gui_action\" value=\"release_notes\">\
                    <label>Out File / Dir</label>\
                    <input type=\"text\" name=\"release_out_dir\" placeholder=\"target/tmp/release-notes.md\">\
                    <label>Channel</label>\
                    <input type=\"text\" name=\"release_channel\" value=\"stable\">\
                    <button type=\"submit\">Release Notes</button>\
                </form>\
                <form method=\"post\" action=\"/command\">\
                    <input type=\"hidden\" name=\"gui_action\" value=\"release_package_local\">\
                    <label>Out Dir</label>\
                    <input type=\"text\" name=\"release_out_dir\" placeholder=\"target/tmp/release-1.0.2\">\
                    <label>Channel</label>\
                    <input type=\"text\" name=\"release_channel\" value=\"stable\">\
                    <label>Rollback From</label>\
                    <input type=\"text\" name=\"release_rollback_from\" placeholder=\"last-stable\">\
                    <div class=\"inline-actions\">\
                        <button type=\"submit\">Package Local</button>\
                        <button type=\"submit\" name=\"gui_action\" value=\"release_package\" class=\"ghost\">Package Release</button>\
                        <button type=\"submit\" name=\"gui_action\" value=\"release_sign\" class=\"ghost\">Sign Artifacts</button>\
                        <button type=\"submit\" name=\"gui_action\" value=\"release_verify\" class=\"ghost\">Verify Release</button>\
                        <button type=\"submit\" name=\"gui_action\" value=\"verify_sprint22\" class=\"ghost\">Verify Sprint22</button>\
                        <button type=\"submit\" name=\"gui_action\" value=\"verify_sprint23\" class=\"ghost\">Verify Sprint23</button>\
                        <button type=\"submit\" name=\"gui_action\" value=\"verify_sprint24\" class=\"ghost\">Verify Sprint24</button>\
                    </div>\
                </form>\
            </div>\
        </section>",
        render_command_button("run ops.check_deps", "Check Deps", "ghost"),
        render_command_button("run ops.dev_check", "Dev Check", "ghost"),
        render_command_button("run release.sign", "Release Sign", "ghost"),
        render_command_button("run release.package", "Release Package", "ghost"),
        render_command_button("ops", "Ops Catalog", "ghost")
    )
}

fn render_command_button(command: &str, label: &str, class_name: &str) -> String {
    format!(
        "<form method=\"post\" action=\"/command\"><input type=\"hidden\" name=\"command\" value=\"{}\"><button class=\"{}\" type=\"submit\">{}</button></form>",
        escape_html(command),
        class_name,
        escape_html(label)
    )
}

fn render_disabled_button(label: &str, class_name: &str) -> String {
    format!(
        "<div class=\"disabled-action\"><button class=\"{}\" type=\"button\" disabled>{}</button></div>",
        class_name,
        escape_html(label)
    )
}

fn render_active_panel(
    runtime: &HostRuntime,
    snapshot: &StoreSnapshot,
    active_view: &str,
) -> String {
    match active_view {
        "empty.state" => render_empty_view(),
        "history.panel" => render_history_view(snapshot),
        "branches.panel" => render_branches_view(snapshot),
        "tags.panel" => render_tags_view(snapshot),
        "compare.panel" => render_compare_view(snapshot),
        "diagnostics.panel" => render_diagnostics_view(snapshot),
        "logs.panel" => render_logs_view(runtime, snapshot),
        _ => render_status_view(snapshot),
    }
}

fn render_empty_view() -> String {
    "<section class=\"card\"><h2>No Repository Opened</h2><p class=\"meta\">Open a Git repository to load status, history, branch management, compare, diff, and diagnostics in the main workspace.</p></section>".to_string()
}

fn render_status_view(snapshot: &StoreSnapshot) -> String {
    let metrics = [
        ("staged", snapshot.status.staged.len().to_string()),
        ("unstaged", snapshot.status.unstaged.len().to_string()),
        ("untracked", snapshot.status.untracked.len().to_string()),
    ]
    .into_iter()
    .map(|(label, value)| format!("<span class=\"badge\">{} {}</span>", label, value))
    .collect::<Vec<_>>()
    .join("");
    format!(
        "<section class=\"card\"><h2>Status</h2><div class=\"hero-metrics\">{}</div></section><section class=\"grid-two\">{}{}{}{}</section>{}",
        metrics,
        render_path_list("Staged", &snapshot.status.staged),
        render_path_list("Unstaged", &snapshot.status.unstaged),
        render_path_list("Untracked", &snapshot.status.untracked),
        if snapshot.status.staged.is_empty()
            && snapshot.status.unstaged.is_empty()
            && snapshot.status.untracked.is_empty()
        {
            "<section class=\"card\"><h2>Workspace</h2><p class=\"meta\">No tracked changes in the current repository state.</p></section>".to_string()
        } else {
            String::new()
        },
        render_diff_card(snapshot)
    )
}

fn render_history_view(snapshot: &StoreSnapshot) -> String {
    let commits = if snapshot.history.commits.is_empty() {
        "<p class=\"meta\">No history loaded yet.</p>".to_string()
    } else {
        let items = snapshot
            .history
            .commits
            .iter()
            .map(|commit| {
                format!(
                    "<li class=\"item\"><div class=\"row\"><form method=\"post\" action=\"/command\"><input type=\"hidden\" name=\"command\" value=\"run history.select_commit {}\"><button type=\"submit\">{}</button></form><span class=\"meta\">{} · {}</span></div><div>{}</div><div class=\"list-actions\">{}{}{}</div></li>",
                    escape_html(&commit.oid),
                    escape_html(&short_oid(&commit.oid)),
                    escape_html(&commit.author),
                    escape_html(&commit.time),
                    escape_html(&commit.summary),
                    render_command_button(
                        &format!("run history.details {}", shell_quote(&commit.oid)),
                        "Details",
                        "ghost"
                    ),
                    render_command_button(
                        &format!("run diff.commit {}", shell_quote(&commit.oid)),
                        "Diff",
                        "ghost"
                    ),
                    render_command_button(
                        &format!("run history.select_commit {}", shell_quote(&commit.oid)),
                        "Select",
                        "ghost"
                    )
                )
            })
            .collect::<Vec<_>>()
            .join("");
        format!("<ul class=\"clean\">{items}</ul>")
    };
    let details = snapshot
        .selection
        .selected_commit_oid
        .as_deref()
        .and_then(|oid| snapshot.commit_cache.get(oid))
        .map(|details| {
            format!(
                "<section class=\"card\"><h2>Commit Details</h2><pre>{}\n{}\n\n{}</pre></section>",
                escape_html(&details.oid),
                escape_html(&details.author),
                escape_html(&details.message)
            )
        })
        .unwrap_or_else(|| {
            "<section class=\"card\"><h2>Commit Details</h2><p class=\"meta\">Select a commit or run `history.details`.</p></section>".to_string()
        });
    format!(
        "<section class=\"card\"><h2>History</h2>{}</section>{}{}",
        commits,
        details,
        render_blame_card(snapshot)
    )
}

fn render_branches_view(snapshot: &StoreSnapshot) -> String {
    let branches = if snapshot.branches.branches.is_empty() {
        "<p class=\"meta\">No branch data loaded yet.</p>".to_string()
    } else {
        let items = snapshot
            .branches
            .branches
            .iter()
            .map(|branch| {
                let marker = if branch.is_current { "current" } else { "branch" };
                format!(
                    "<li class=\"item\"><div class=\"row\"><form method=\"post\" action=\"/command\"><input type=\"hidden\" name=\"command\" value=\"select branch {}\"><button type=\"submit\">{}</button></form><span class=\"badge\">{}</span></div><div class=\"meta\">upstream: {}</div><div class=\"list-actions\">{}{}{}</div></li>",
                    escape_html(&branch.name),
                    escape_html(&branch.name),
                    marker,
                    escape_html(branch.upstream.as_deref().unwrap_or("<none>")),
                    render_command_button(
                        &format!("run branch.checkout {}", shell_quote(&branch.name)),
                        "Checkout",
                        "ghost"
                    ),
                    render_command_button(
                        &format!("run compare.refs HEAD {}", shell_quote(&branch.name)),
                        "Compare",
                        "ghost"
                    ),
                    if branch.is_current {
                        String::new()
                    } else {
                        render_command_button(
                            &format!("run --confirm branch.delete {}", shell_quote(&branch.name)),
                            "Delete",
                            "secondary"
                        )
                    }
                )
            })
            .collect::<Vec<_>>()
            .join("");
        format!("<ul class=\"clean\">{items}</ul>")
    };
    let rebase = snapshot
        .rebase
        .plan
        .as_ref()
        .map(|plan| {
            let entries = plan
                .entries
                .iter()
                .enumerate()
                .map(|(index, entry)| {
                    let action_buttons = [
                        ("pick", "Pick"),
                        ("reword", "Reword"),
                        ("edit", "Edit"),
                        ("squash", "Squash"),
                        ("fixup", "Fixup"),
                        ("drop", "Drop"),
                    ]
                    .into_iter()
                    .map(|(action, label)| {
                        format!(
                            "<form method=\"post\" action=\"/command\"><input type=\"hidden\" name=\"gui_action\" value=\"rebase_set_action\"><input type=\"hidden\" name=\"entry_index\" value=\"{}\"><input type=\"hidden\" name=\"rebase_action\" value=\"{}\"><button class=\"ghost\" type=\"submit\">{}</button></form>",
                            index,
                            action,
                            label
                        )
                    })
                    .collect::<Vec<_>>()
                    .join("");
                    let move_controls = format!(
                        "{}{}",
                        if index > 0 {
                            format!(
                                "<form method=\"post\" action=\"/command\"><input type=\"hidden\" name=\"gui_action\" value=\"rebase_move\"><input type=\"hidden\" name=\"from_index\" value=\"{}\"><input type=\"hidden\" name=\"to_index\" value=\"{}\"><button class=\"ghost\" type=\"submit\">Move Up</button></form>",
                                index,
                                index - 1
                            )
                        } else {
                            String::new()
                        },
                        if index + 1 < plan.entries.len() {
                            format!(
                                "<form method=\"post\" action=\"/command\"><input type=\"hidden\" name=\"gui_action\" value=\"rebase_move\"><input type=\"hidden\" name=\"from_index\" value=\"{}\"><input type=\"hidden\" name=\"to_index\" value=\"{}\"><button class=\"ghost\" type=\"submit\">Move Down</button></form>",
                                index,
                                index + 1
                            )
                        } else {
                            String::new()
                        }
                    );
                    let warnings = if entry.warnings.is_empty() {
                        String::new()
                    } else {
                        format!(
                            "<div class=\"meta\">warnings: {}</div>",
                            escape_html(&entry.warnings.join(", "))
                        )
                    };
                    format!(
                        "<li class=\"item\"><div><strong>{}</strong> {} {}</div><div class=\"meta\">{}</div>{}<div class=\"list-actions\">{}{}</div></li>",
                        short_oid(&entry.oid),
                        action_label(&entry.action),
                        escape_html(&entry.summary),
                        index,
                        warnings,
                        action_buttons,
                        move_controls
                    )
                })
                .collect::<Vec<_>>()
                .join("");
            format!(
                "<section class=\"card\"><h2>Rebase Plan</h2><pre>base: {}\ncommits: {}\nautosquash: {}</pre><ul class=\"clean\">{}</ul></section>",
                escape_html(&plan.base_ref),
                plan.affected_commit_count,
                plan.autosquash_aware,
                entries
            )
        })
        .unwrap_or_else(|| {
            "<section class=\"card\"><h2>Rebase Plan</h2><p class=\"meta\">No active rebase plan.</p></section>".to_string()
        });
    let conflict = snapshot
        .repo
        .as_ref()
        .and_then(|repo| repo.conflict_state.as_ref())
        .map(|state| {
            format!(
                "<section class=\"card\"><h2>Conflict State</h2><pre>{}</pre></section>",
                escape_html(&format!("{state:?}"))
            )
        })
        .unwrap_or_default();
    format!(
        "<section class=\"card\"><h2>Branches</h2>{}</section>{}{}{}",
        branches,
        rebase,
        conflict,
        render_diff_card(snapshot)
    )
}

fn render_tags_view(snapshot: &StoreSnapshot) -> String {
    let tags = if snapshot.tags.tags.is_empty() {
        "<p class=\"meta\">No tags loaded yet.</p>".to_string()
    } else {
        let items = snapshot
            .tags
            .tags
            .iter()
            .map(|tag| {
                format!(
                    "<li class=\"item\"><div>{}</div><div class=\"list-actions\">{}{}</div></li>",
                    escape_html(&tag.name),
                    render_command_button(
                        &format!("run tag.checkout {}", shell_quote(&tag.name)),
                        "Checkout",
                        "ghost"
                    ),
                    render_command_button(
                        &format!("run --confirm tag.delete {}", shell_quote(&tag.name)),
                        "Delete",
                        "secondary"
                    )
                )
            })
            .collect::<Vec<_>>()
            .join("");
        format!("<ul class=\"clean\">{items}</ul>")
    };
    format!("<section class=\"card\"><h2>Tags</h2>{tags}</section>")
}

fn render_compare_view(snapshot: &StoreSnapshot) -> String {
    let summary = format!(
        "base: {}\nhead: {}\nahead: {}\nbehind: {}",
        display_value(snapshot.compare.base_ref.as_deref().unwrap_or("")),
        display_value(snapshot.compare.head_ref.as_deref().unwrap_or("")),
        snapshot.compare.ahead,
        snapshot.compare.behind
    );
    let commits = if snapshot.compare.commits.is_empty() {
        "<p class=\"meta\">No compare commits loaded yet.</p>".to_string()
    } else {
        let items = snapshot
            .compare
            .commits
            .iter()
            .map(|commit| {
                format!(
                    "<li class=\"item\"><span class=\"meta\">{}</span><div>{}</div></li>",
                    escape_html(&short_oid(&commit.oid)),
                    escape_html(&commit.summary)
                )
            })
            .collect::<Vec<_>>()
            .join("");
        format!("<ul class=\"clean\">{items}</ul>")
    };
    format!(
        "<section class=\"card\"><h2>Compare</h2><pre>{}</pre>{}</section>{}",
        escape_html(&summary),
        commits,
        render_diff_card(snapshot)
    )
}

fn render_diagnostics_view(snapshot: &StoreSnapshot) -> String {
    let running = snapshot
        .journal
        .entries
        .iter()
        .filter(|entry| matches!(entry.status, JournalStatus::Started))
        .count();
    let succeeded = snapshot
        .journal
        .entries
        .iter()
        .filter(|entry| matches!(entry.status, JournalStatus::Succeeded))
        .count();
    let failed = snapshot
        .journal
        .entries
        .iter()
        .filter(|entry| matches!(entry.status, JournalStatus::Failed))
        .count();
    let summary = format!(
        "journal entries: {}\nrunning: {}\nsucceeded: {}\nfailed: {}\nlfs: {}\nselected plugin: {}",
        snapshot.journal.entries.len(),
        running,
        succeeded,
        failed,
        match snapshot.repo_capabilities.as_ref() {
            Some(caps) if !caps.lfs_available => "unavailable (git-lfs missing)",
            Some(caps) if caps.lfs_detected => "available (repo detected)",
            Some(_) => "available (repo not detected)",
            None => "<unknown>",
        },
        snapshot
            .selection
            .selected_plugin_id
            .as_deref()
            .unwrap_or("<none>")
    );
    let plugins = if snapshot.installed_plugins.is_empty() {
        "<p class=\"meta\">No installed plugins.</p>".to_string()
    } else {
        let items = snapshot
            .installed_plugins
            .iter()
            .map(|plugin| {
                format!(
                    "<li class=\"item\"><div class=\"row\"><form method=\"post\" action=\"/command\"><input type=\"hidden\" name=\"command\" value=\"select plugin {}\"><button type=\"submit\">{}</button></form><span class=\"badge\">v{}</span></div><div class=\"meta\">enabled={} · protocol={} · perms={}</div><div class=\"list-actions\">{}{}{}</div></li>",
                    escape_html(&plugin.plugin_id),
                    escape_html(&plugin.plugin_id),
                    escape_html(&plugin.version),
                    plugin.enabled,
                    escape_html(&plugin.protocol_version),
                    escape_html(&plugin.permissions.join(", ")),
                    render_command_button(
                        &format!("run plugin.enable {}", shell_quote(&plugin.plugin_id)),
                        "Enable",
                        "ghost"
                    ),
                    render_command_button(
                        &format!("run plugin.disable {}", shell_quote(&plugin.plugin_id)),
                        "Disable",
                        "ghost"
                    ),
                    render_command_button(
                        &format!("run --confirm plugin.remove {}", shell_quote(&plugin.plugin_id)),
                        "Remove",
                        "secondary"
                    )
                )
            })
            .collect::<Vec<_>>()
            .join("");
        format!("<ul class=\"clean\">{items}</ul>")
    };
    let runtime_plugins = if snapshot.plugins.is_empty() {
        "<p class=\"meta\">No runtime plugin status available.</p>".to_string()
    } else {
        let items = snapshot
            .plugins
            .iter()
            .map(|plugin| match &plugin.health {
                PluginHealth::Ready => format!(
                    "<li class=\"item\"><div>{}</div><div class=\"meta\">ready</div><div class=\"list-actions\">{}</div></li>",
                    escape_html(&plugin.plugin_id),
                    render_command_button(
                        &format!("select plugin {}", shell_quote(&plugin.plugin_id)),
                        "Select",
                        "ghost"
                    )
                ),
                PluginHealth::Unavailable { message } => format!(
                    "<li class=\"item\"><div>{}</div><div class=\"meta\">unavailable: {}</div><div class=\"list-actions\">{}</div></li>",
                    escape_html(&plugin.plugin_id),
                    escape_html(message),
                    render_command_button(
                        &format!("select plugin {}", shell_quote(&plugin.plugin_id)),
                        "Select",
                        "ghost"
                    )
                ),
            })
            .collect::<Vec<_>>()
            .join("");
        format!("<ul class=\"clean\">{items}</ul>")
    };
    format!(
        "<section class=\"grid-two\"><section class=\"card\"><h2>Diagnostics Summary</h2><pre>{}</pre></section><section class=\"card\"><h2>Installed Plugins</h2>{}</section></section><section class=\"card\"><h2>Runtime Plugin Health</h2>{}</section>",
        escape_html(&summary),
        plugins,
        runtime_plugins
    )
}

fn render_logs_view(runtime: &HostRuntime, snapshot: &StoreSnapshot) -> String {
    let journal_entries = snapshot
        .journal
        .entries
        .iter()
        .rev()
        .take(12)
        .map(|entry| {
            let status = match entry.status {
                JournalStatus::Started => "running",
                JournalStatus::Succeeded => "ok",
                JournalStatus::Failed => "failed",
            };
            format!(
                "<li class=\"item\"><div class=\"row\"><span class=\"badge\">{}</span><span class=\"meta\">#{}</span></div><div>{}</div><div class=\"meta\">{}</div></li>",
                status,
                entry.id,
                escape_html(&entry.op),
                escape_html(entry.error.as_deref().unwrap_or(""))
            )
        })
        .collect::<Vec<_>>()
        .join("");
    format!(
        "<section class=\"grid-two\"><section class=\"card\"><h2>System Logs</h2><ul class=\"clean\">{}</ul></section><section class=\"card\"><h2>Runtime Snapshot</h2><pre>{}</pre></section></section><section class=\"grid-two\"><section class=\"card\"><h2>Action Catalog</h2><pre>{}</pre></section><section class=\"card\"><h2>Ops Catalog</h2><pre>{}</pre></section></section>",
        journal_entries,
        escape_html(&runtime.render_screen()),
        escape_html(&runtime.render_actions()),
        escape_html(&runtime.ops_catalog())
    )
}

fn render_path_list(title: &str, paths: &[String]) -> String {
    let body = if paths.is_empty() {
        "<p class=\"meta\">None</p>".to_string()
    } else {
        let items = paths
            .iter()
            .map(|path| {
                let primary = match title {
                    "Staged" => render_command_button(
                        &format!("run index.unstage_paths {}", shell_quote(path)),
                        "Unstage",
                        "ghost",
                    ),
                    _ => render_command_button(
                        &format!("run index.stage_paths {}", shell_quote(path)),
                        "Stage",
                        "ghost",
                    ),
                };
                let diff_command = if title == "Staged" {
                    format!("run diff.index {}", shell_quote(path))
                } else {
                    format!("run diff.worktree {}", shell_quote(path))
                };
                format!(
                    "<li class=\"item\"><div class=\"row\"><form method=\"post\" action=\"/command\"><input type=\"hidden\" name=\"command\" value=\"select file {}\"><button type=\"submit\">Select</button></form><span>{}</span></div><div class=\"list-actions\">{}{}{}{}</div></li>",
                    escape_html(&shell_quote(path)),
                    escape_html(path),
                    primary,
                    render_command_button(&diff_command, "Diff", "ghost"),
                    render_command_button(
                        &format!("run history.file {}", shell_quote(path)),
                        "History",
                        "ghost",
                    ),
                    render_command_button(
                        &format!("run blame.file {}", shell_quote(path)),
                        "Blame",
                        "ghost",
                    )
                )
            })
            .collect::<Vec<_>>()
            .join("");
        format!("<ul class=\"clean\">{items}</ul>")
    };
    format!(
        "<section class=\"card\"><h2>{}</h2>{}</section>",
        title, body
    )
}

fn render_diff_card(snapshot: &StoreSnapshot) -> String {
    let diff_content = snapshot
        .diff
        .content
        .as_deref()
        .unwrap_or("No diff loaded.");
    let diff_source = snapshot
        .diff
        .source
        .as_ref()
        .map(diff_source_label)
        .unwrap_or("none");
    let hunk_cards = render_diff_hunks(snapshot);
    format!(
        "<section class=\"card\"><h2>Diff</h2><div class=\"hero-metrics\"><span class=\"badge\">source {}</span><span class=\"badge\">hunks {}</span></div><pre>{}</pre>{}</section>",
        escape_html(diff_source),
        snapshot.diff.hunks.len(),
        escape_html(diff_content),
        hunk_cards
    )
}

fn render_diff_hunks(snapshot: &StoreSnapshot) -> String {
    if snapshot.diff.hunks.is_empty() {
        return String::new();
    }

    let cards = snapshot
        .diff
        .hunks
        .iter()
        .map(|hunk| {
            let lines = hunk
                .lines
                .iter()
                .enumerate()
                .map(|(index, line)| format!("{:>2} | {}", index, line))
                .collect::<Vec<_>>()
                .join("\n");
            format!(
                "<div class=\"hunk-card\"><div class=\"hunk-meta\"><strong>{}</strong><div class=\"meta\">hunk #{} · {}</div></div><div class=\"list-actions\">{}</div><form method=\"post\" action=\"/command\" class=\"stack\"><input type=\"hidden\" name=\"diff_path\" value=\"{}\"><input type=\"hidden\" name=\"diff_hunk_index\" value=\"{}\"><label>Line Indices</label><input type=\"text\" name=\"line_indices\" placeholder=\"0 1 2\"><div class=\"inline-actions\">{}</div></form><pre class=\"line-guide\">{}</pre></div>",
                escape_html(&hunk.file_path),
                hunk.hunk_index,
                escape_html(&hunk.header),
                render_diff_hunk_buttons(snapshot, hunk),
                escape_html(&hunk.file_path),
                hunk.hunk_index,
                render_diff_line_buttons(snapshot),
                escape_html(&lines)
            )
        })
        .collect::<Vec<_>>()
        .join("");

    format!("<div class=\"hunk-list\">{cards}</div>")
}

fn render_diff_hunk_buttons(snapshot: &StoreSnapshot, hunk: &state_store::DiffHunk) -> String {
    match snapshot.diff.source.as_ref() {
        Some(DiffSource::Worktree { .. }) => [
            render_diff_hunk_form(
                "diff_hunk_stage",
                &hunk.file_path,
                hunk.hunk_index,
                "Stage Hunk",
            ),
            render_diff_hunk_form(
                "diff_hunk_discard",
                &hunk.file_path,
                hunk.hunk_index,
                "Discard Hunk",
            ),
        ]
        .join(""),
        Some(DiffSource::Index { .. }) => [render_diff_hunk_form(
            "diff_hunk_unstage",
            &hunk.file_path,
            hunk.hunk_index,
            "Unstage Hunk",
        )]
        .join(""),
        _ => String::new(),
    }
}

fn render_diff_line_buttons(snapshot: &StoreSnapshot) -> String {
    match snapshot.diff.source.as_ref() {
        Some(DiffSource::Worktree { .. }) => [
            render_diff_line_form_button("diff_lines_stage", "Stage Lines"),
            render_diff_line_form_button("diff_lines_discard", "Discard Lines"),
        ]
        .join(""),
        Some(DiffSource::Index { .. }) => [render_diff_line_form_button(
            "diff_lines_unstage",
            "Unstage Lines",
        )]
        .join(""),
        _ => String::new(),
    }
}

fn render_diff_hunk_form(gui_action: &str, path: &str, hunk_index: usize, label: &str) -> String {
    format!(
        "<form method=\"post\" action=\"/command\"><input type=\"hidden\" name=\"gui_action\" value=\"{}\"><input type=\"hidden\" name=\"diff_path\" value=\"{}\"><input type=\"hidden\" name=\"diff_hunk_index\" value=\"{}\"><button class=\"ghost\" type=\"submit\">{}</button></form>",
        gui_action,
        escape_html(path),
        hunk_index,
        escape_html(label)
    )
}

fn render_diff_line_form_button(gui_action: &str, label: &str) -> String {
    format!(
        "<button class=\"ghost\" type=\"submit\" name=\"gui_action\" value=\"{}\">{}</button>",
        gui_action,
        escape_html(label)
    )
}

fn diff_source_label(source: &DiffSource) -> &'static str {
    match source {
        DiffSource::Worktree { .. } => "worktree",
        DiffSource::Index { .. } => "index",
        DiffSource::Commit { .. } => "commit",
        DiffSource::Compare { .. } => "compare",
    }
}

fn render_blame_card(snapshot: &StoreSnapshot) -> String {
    if snapshot.blame.lines.is_empty() {
        return "<section class=\"card\"><h2>Blame</h2><p class=\"meta\">No blame loaded.</p></section>".to_string();
    }
    let lines = snapshot
        .blame
        .lines
        .iter()
        .take(120)
        .map(|line| {
            format!(
                "{} {} | {}",
                line.line_no,
                short_oid(&line.oid),
                line.content
            )
        })
        .collect::<Vec<_>>()
        .join("\n");
    format!(
        "<section class=\"card\"><h2>Blame</h2><pre>{}</pre></section>",
        escape_html(&lines)
    )
}

fn action_label(action: &state_store::RebaseEntryAction) -> &'static str {
    match action {
        state_store::RebaseEntryAction::Pick => "pick",
        state_store::RebaseEntryAction::Reword => "reword",
        state_store::RebaseEntryAction::Edit => "edit",
        state_store::RebaseEntryAction::Squash => "squash",
        state_store::RebaseEntryAction::Fixup => "fixup",
        state_store::RebaseEntryAction::Drop => "drop",
    }
}

fn short_oid(oid: &str) -> String {
    oid.chars().take(8).collect()
}

fn display_value(value: &str) -> String {
    if value.is_empty() {
        "&lt;none&gt;".to_string()
    } else {
        escape_html(value)
    }
}

fn escape_html(value: &str) -> String {
    value
        .replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn unique_temp_dir(label: &str) -> std::path::PathBuf {
        static COUNTER: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_nanos())
            .unwrap_or(0);
        let seq = COUNTER.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        std::env::temp_dir().join(format!("branchforge-gui-{label}-{nanos}-{seq}"))
    }

    fn test_runtime(root: &std::path::Path) -> HostRuntime {
        HostRuntime::new(ConsoleRunnerConfig {
            cwd: root.to_path_buf(),
            plugins_root: root.join("plugins"),
            auto_render: false,
        })
    }

    fn assert_no_nested_forms(html: &str) {
        let mut depth = 0usize;
        let mut cursor = html;

        loop {
            let next_open = cursor.find("<form");
            let next_close = cursor.find("</form>");
            match (next_open, next_close) {
                (Some(open), Some(close)) if open < close => {
                    depth += 1;
                    assert_eq!(depth, 1, "nested <form> detected");
                    cursor = &cursor[open + 5..];
                }
                (_, Some(close)) => {
                    depth = depth.checked_sub(1).expect("unexpected </form>");
                    cursor = &cursor[close + "</form>".len()..];
                }
                (Some(open), None) => {
                    depth += 1;
                    assert_eq!(depth, 1, "nested <form> detected");
                    cursor = &cursor[open + 5..];
                }
                (None, None) => break,
            }
        }

        assert_eq!(depth, 0, "all forms should be closed");
    }

    #[test]
    fn decodes_form_urlencoded_payload() {
        let form = parse_form_urlencoded(b"command=run+history.page+0+20&repo_path=foo%2Fbar");
        assert_eq!(
            form.get("command").map(String::as_str),
            Some("run history.page 0 20")
        );
        assert_eq!(form.get("repo_path").map(String::as_str), Some("foo/bar"));
    }

    #[test]
    fn parses_content_length_case_insensitively() {
        assert_eq!(
            parse_content_length_header("Content-Length: 42\r\n"),
            Some(42)
        );
        assert_eq!(
            parse_content_length_header("content-length: 42\r\n"),
            Some(42)
        );
        assert_eq!(
            parse_content_length_header("CONTENT-LENGTH: 42\r\n"),
            Some(42)
        );
        assert_eq!(parse_content_length_header("Host: localhost\r\n"), None);
    }

    #[test]
    fn shell_quote_wraps_paths_with_spaces() {
        assert_eq!(shell_quote("src/lib.rs"), "src/lib.rs");
        assert_eq!(shell_quote("docs/with space.md"), "\"docs/with space.md\"");
    }

    #[test]
    fn workflow_widgets_build_runtime_commands() {
        let mut form = HashMap::new();
        form.insert("gui_action".to_string(), "commit_create".to_string());
        form.insert("commit_message".to_string(), "ship gui".to_string());
        assert_eq!(
            build_command_from_form(&form).as_deref(),
            Some("run commit.create \"ship gui\"")
        );

        let mut form = HashMap::new();
        form.insert("gui_action".to_string(), "branch_create".to_string());
        form.insert("branch_name".to_string(), "feature/gui".to_string());
        form.insert("branch_base".to_string(), "main".to_string());
        assert_eq!(
            build_command_from_form(&form).as_deref(),
            Some("run branch.create feature/gui main")
        );

        let mut form = HashMap::new();
        form.insert("gui_action".to_string(), "compare_refs".to_string());
        form.insert("compare_base".to_string(), "main".to_string());
        form.insert("compare_head".to_string(), "feature/gui".to_string());
        assert_eq!(
            build_command_from_form(&form).as_deref(),
            Some("run compare.refs main feature/gui")
        );
    }

    #[test]
    fn advanced_widgets_build_runtime_commands() {
        let mut form = HashMap::new();
        form.insert("gui_action".to_string(), "rebase_interactive".to_string());
        form.insert("rebase_base_ref".to_string(), "main".to_string());
        form.insert("rebase_autosquash".to_string(), "autosquash".to_string());
        assert_eq!(
            build_command_from_form(&form).as_deref(),
            Some("run --confirm rebase.interactive main autosquash")
        );

        let mut form = HashMap::new();
        form.insert(
            "gui_action".to_string(),
            "plugin_install_registry".to_string(),
        );
        form.insert("plugin_id".to_string(), "sample_status".to_string());
        form.insert(
            "registry_path".to_string(),
            "plugin_registry/registry.json".to_string(),
        );
        assert_eq!(
            build_command_from_form(&form).as_deref(),
            Some("run plugin.install_registry sample_status plugin_registry/registry.json")
        );

        let mut form = HashMap::new();
        form.insert("gui_action".to_string(), "diff_lines_stage".to_string());
        form.insert("diff_path".to_string(), "src/lib.rs".to_string());
        form.insert("diff_hunk_index".to_string(), "2".to_string());
        form.insert("line_indices".to_string(), "0, 2 4".to_string());
        assert_eq!(
            build_command_from_form(&form).as_deref(),
            Some("run index.stage_lines src/lib.rs 2 0 2 4")
        );

        let mut form = HashMap::new();
        form.insert("gui_action".to_string(), "history_search".to_string());
        form.insert("history_author".to_string(), "Dev User".to_string());
        form.insert("history_text".to_string(), "gui".to_string());
        form.insert("history_limit".to_string(), "30".to_string());
        assert_eq!(
            build_command_from_form(&form).as_deref(),
            Some("run history.search 0 30 \"Dev User\" gui")
        );

        let mut form = HashMap::new();
        form.insert("gui_action".to_string(), "merge_execute".to_string());
        form.insert("merge_source_ref".to_string(), "feature/gui".to_string());
        form.insert("merge_mode".to_string(), "no-ff".to_string());
        assert_eq!(
            build_command_from_form(&form).as_deref(),
            Some("run --confirm merge.execute feature/gui no-ff")
        );

        let mut form = HashMap::new();
        form.insert("gui_action".to_string(), "diff_hunk_discard".to_string());
        form.insert("diff_path".to_string(), "src/lib.rs".to_string());
        form.insert("diff_hunk_index".to_string(), "2".to_string());
        assert_eq!(
            build_command_from_form(&form).as_deref(),
            Some("run --confirm file.discard_hunk src/lib.rs 2")
        );

        let mut form = HashMap::new();
        form.insert("gui_action".to_string(), "diff_lines_discard".to_string());
        form.insert("diff_path".to_string(), "src/lib.rs".to_string());
        form.insert("diff_hunk_index".to_string(), "2".to_string());
        form.insert("line_indices".to_string(), "0, 2 4".to_string());
        assert_eq!(
            build_command_from_form(&form).as_deref(),
            Some("run --confirm file.discard_lines src/lib.rs 2 0 2 4")
        );

        let mut form = HashMap::new();
        form.insert("gui_action".to_string(), "release_package".to_string());
        form.insert(
            "release_out_dir".to_string(),
            "target/tmp/release-1.0.2".to_string(),
        );
        form.insert("release_channel".to_string(), "stable".to_string());
        form.insert(
            "release_rollback_from".to_string(),
            "last-stable".to_string(),
        );
        assert_eq!(
            build_command_from_form(&form).as_deref(),
            Some("run release.package target/tmp/release-1.0.2 stable last-stable")
        );
    }

    #[test]
    fn root_page_renders_gui_shell() {
        let root = unique_temp_dir("render");
        assert!(std::fs::create_dir_all(&root).is_ok());
        let runtime = test_runtime(&root);

        let page = render_page(&runtime, Some("ready"), None);
        assert!(page.contains("Branchforge"));
        assert!(page.contains("Runtime GUI"));
        assert!(page.contains("Quick Actions"));
        assert!(page.contains("Primary Flows"));
        assert!(page.contains("Open a repository to unlock runtime panels."));
        assert!(page.contains("panel logs"));
        assert!(page.contains("No Repository Opened"));
        assert!(!page.contains("Workspace Controls"));
        assert!(page.contains("id=\"masthead\""));
        assert!(page.contains("[\"masthead\", \"sidebar\", \"content\"]"));
        assert!(page.contains("no repo"));

        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn logs_panel_renders_system_cards_without_repo() {
        let root = unique_temp_dir("logs");
        assert!(std::fs::create_dir_all(&root).is_ok());
        let mut runtime = test_runtime(&root);

        let response = route_request(
            &mut runtime,
            HttpRequest {
                method: "POST".to_string(),
                path: "/command".to_string(),
                body: b"command=panel+logs".to_vec(),
            },
        );

        assert_eq!(response.status, "200 OK");
        assert!(response.body.contains("System Logs"));
        assert!(response.body.contains("Runtime Snapshot"));
        assert!(response.body.contains("Action Catalog"));
        assert!(response.body.contains("Ops Catalog"));

        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn diagnostics_controls_disable_lfs_actions_when_git_lfs_is_missing() {
        let snapshot = StoreSnapshot {
            repo_capabilities: Some(state_store::RepoCapabilitiesSnapshot {
                is_linked_worktree: false,
                has_submodules: false,
                lfs_detected: false,
                lfs_available: false,
            }),
            ..StoreSnapshot::default()
        };

        let controls = render_diagnostics_controls(&snapshot);
        assert!(controls.contains("Install git-lfs to enable LFS actions on this machine."));
        assert!(controls.contains("type=\"button\" disabled"));
        assert!(!controls.contains("run diagnostics.lfs_status"));
        assert!(!controls.contains("run diagnostics.lfs_fetch"));
        assert!(!controls.contains("run diagnostics.lfs_pull"));
    }

    #[test]
    fn diagnostics_summary_reports_missing_git_lfs() {
        let snapshot = StoreSnapshot {
            repo_capabilities: Some(state_store::RepoCapabilitiesSnapshot {
                is_linked_worktree: false,
                has_submodules: false,
                lfs_detected: false,
                lfs_available: false,
            }),
            ..StoreSnapshot::default()
        };

        let page = render_diagnostics_view(&snapshot);
        assert!(page.contains("lfs: unavailable (git-lfs missing)"));
    }

    #[test]
    fn opened_repo_page_does_not_nest_forms() {
        let root = unique_temp_dir("nested-forms");
        let repo_dir = root.join("repo");
        assert!(std::fs::create_dir_all(&repo_dir).is_ok());
        assert!(git_service::run_git(&repo_dir, &["init"]).is_ok());
        assert!(
            git_service::run_git(&repo_dir, &["config", "user.email", "dev@example.com"]).is_ok()
        );
        assert!(git_service::run_git(&repo_dir, &["config", "user.name", "Dev User"]).is_ok());

        let mut runtime = test_runtime(&root);
        let response = route_request(
            &mut runtime,
            HttpRequest {
                method: "POST".to_string(),
                path: "/command".to_string(),
                body: format!(
                    "repo_path={}",
                    repo_dir.to_string_lossy().replace('/', "%2F")
                )
                .into_bytes(),
            },
        );
        assert_eq!(response.status, "200 OK");

        let page = render_page(&runtime, None, None);
        assert_no_nested_forms(&page);
        assert!(page.contains("run --confirm file.discard"));

        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn command_post_updates_runtime_state() {
        let root = unique_temp_dir("command");
        let repo_dir = root.join("repo");
        assert!(std::fs::create_dir_all(&repo_dir).is_ok());
        assert!(git_service::run_git(&repo_dir, &["init"]).is_ok());
        assert!(
            git_service::run_git(&repo_dir, &["config", "user.email", "dev@example.com"]).is_ok()
        );
        assert!(git_service::run_git(&repo_dir, &["config", "user.name", "Dev User"]).is_ok());

        let mut runtime = test_runtime(&root);
        let response = route_request(
            &mut runtime,
            HttpRequest {
                method: "POST".to_string(),
                path: "/command".to_string(),
                body: format!(
                    "repo_path={}",
                    repo_dir.to_string_lossy().replace('/', "%2F")
                )
                .into_bytes(),
            },
        );

        assert_eq!(response.status, "200 OK");
        assert!(response.body.contains("opened repository"));
        assert!(response.body.contains(repo_dir.to_string_lossy().as_ref()));

        let _ = std::fs::remove_dir_all(&root);
    }
}
