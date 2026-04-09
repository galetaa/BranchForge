use std::path::{Path, PathBuf};
use std::process::Command;

use plugin_api::{ActionSpec, ViewSpec};
use plugin_host::{
    PluginProcess, PluginProcessConfig, RestartPolicy, bootstrap_plugin_runtime,
    branches_registration_payload, compare_registration_payload, diagnostics_registration_payload,
    history_registration_payload, repo_manager_registration_payload, status_registration_payload,
    tags_registration_payload,
};

#[test]
fn bundled_plugin_binaries_match_internal_registration_payloads() {
    let repo_root = workspace_root();
    let build_status = Command::new("cargo")
        .args([
            "build",
            "--offline",
            "-p",
            "repo_manager",
            "-p",
            "status",
            "-p",
            "history",
            "-p",
            "branches",
            "-p",
            "tags",
            "-p",
            "compare",
            "-p",
            "diagnostics",
        ])
        .current_dir(&repo_root)
        .status()
        .expect("build bundled plugins");
    assert!(build_status.success());

    let expected = [
        ("repo_manager", repo_manager_registration_payload()),
        ("status", status_registration_payload()),
        ("history", history_registration_payload()),
        ("branches", branches_registration_payload()),
        ("tags", tags_registration_payload()),
        ("compare", compare_registration_payload()),
        ("diagnostics", diagnostics_registration_payload()),
    ];

    for (plugin_id, payload) in expected {
        let binary = resolve_binary(&repo_root, plugin_id);
        let mut process = PluginProcess::spawn(PluginProcessConfig {
            plugin_id: plugin_id.to_string(),
            program: binary.display().to_string(),
            args: Vec::new(),
            restart_policy: RestartPolicy::Never,
        })
        .expect("spawn bundled plugin");
        let runtime = bootstrap_plugin_runtime(&mut process).expect("bootstrap bundled plugin");

        assert_eq!(
            sort_actions(runtime.register.actions),
            sort_actions(payload.actions),
            "action mismatch for {plugin_id}"
        );
        assert_eq!(
            sort_views(runtime.register.views),
            sort_views(payload.views),
            "view mismatch for {plugin_id}"
        );

        process.shutdown().expect("shutdown bundled plugin");
    }
}

fn workspace_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(Path::parent)
        .expect("workspace root")
        .to_path_buf()
}

fn resolve_binary(repo_root: &Path, binary: &str) -> PathBuf {
    let unix = repo_root.join("target").join("debug").join(binary);
    if unix.exists() {
        return unix;
    }
    repo_root
        .join("target")
        .join("debug")
        .join(format!("{binary}.exe"))
}

fn sort_actions(mut actions: Vec<ActionSpec>) -> Vec<ActionSpec> {
    actions.sort_by(|left, right| left.action_id.cmp(&right.action_id));
    actions
}

fn sort_views(mut views: Vec<ViewSpec>) -> Vec<ViewSpec> {
    views.sort_by(|left, right| left.view_id.cmp(&right.view_id));
    views
}
