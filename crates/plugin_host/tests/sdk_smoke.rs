use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::Instant;

use plugin_api::ActionContext;
use plugin_host::{
    PluginProcess, PluginProcessConfig, RestartPolicy, bootstrap_plugin_runtime,
    invoke_plugin_action,
};

#[test]
fn sample_external_plugin_builds_and_runs() {
    let manifest_dir = match std::env::var("CARGO_MANIFEST_DIR") {
        Ok(value) => PathBuf::from(value),
        Err(_) => return,
    };
    let repo_root = match manifest_dir.parent().and_then(|path| path.parent()) {
        Some(root) => root.to_path_buf(),
        None => return,
    };

    let manifest = repo_root.join("external_plugins/sample_plugin/Cargo.toml");
    let manifest_str = match manifest.to_str() {
        Some(path) => path,
        None => return,
    };

    let build = Command::new("cargo")
        .args(["build", "--offline", "--manifest-path", manifest_str])
        .current_dir(&repo_root)
        .status();
    assert!(build.is_ok());
    if let Ok(status) = build {
        assert!(status.success());
    }

    let exe_path = resolve_sample_binary(&repo_root);
    let mut process = PluginProcess::spawn(PluginProcessConfig {
        plugin_id: "sample_external".to_string(),
        program: exe_path.display().to_string(),
        args: Vec::new(),
        restart_policy: RestartPolicy::Never,
    })
    .expect("spawn sample plugin");
    let mut runtime = bootstrap_plugin_runtime(&mut process).expect("bootstrap sample plugin");
    assert!(
        runtime
            .register
            .actions
            .iter()
            .any(|action| action.action_id == "sample.ping")
    );

    let result = invoke_plugin_action(
        &mut process,
        &mut runtime.session,
        "sample.ping",
        ActionContext {
            selection_files: vec!["README.md".to_string()],
        },
        Instant::now(),
    )
    .expect("invoke sample plugin");
    assert_eq!(result["plugin_id"], "sample_external");
    assert_eq!(result["action_id"], "sample.ping");
    assert_eq!(result["selection_files"][0], "README.md");

    process.shutdown().expect("shutdown sample plugin");
}

fn resolve_sample_binary(repo_root: &Path) -> PathBuf {
    let base = repo_root.join("external_plugins/sample_plugin/target/debug/sample_external_plugin");
    if base.exists() {
        return base;
    }

    repo_root.join("external_plugins/sample_plugin/target/debug/sample_external_plugin.exe")
}
