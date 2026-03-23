use std::path::{Path, PathBuf};
use std::process::Command;

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
    let output = Command::new(&exe_path).output();
    assert!(output.is_ok());
    if let Ok(out) = output {
        let stdout = String::from_utf8_lossy(&out.stdout);
        assert!(stdout.contains("plugin.hello"));
        assert!(stdout.contains("plugin.register"));
    }
}

fn resolve_sample_binary(repo_root: &Path) -> PathBuf {
    let base = repo_root.join("external_plugins/sample_plugin/target/debug/sample_external_plugin");
    if base.exists() {
        return base;
    }

    repo_root.join("external_plugins/sample_plugin/target/debug/sample_external_plugin.exe")
}
