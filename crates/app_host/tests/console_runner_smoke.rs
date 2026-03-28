use std::io::Write;
use std::process::{Command, Stdio};

fn unique_temp_dir(label: &str) -> std::path::PathBuf {
    static COUNTER: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0);
    let seq = COUNTER.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    std::env::temp_dir().join(format!("branchforge-console-smoke-{label}-{nanos}-{seq}"))
}

#[test]
fn console_runner_supports_open_actions_run_show_and_quit() {
    let root = unique_temp_dir("root");
    let repo_dir = root.join("repo");
    let plugins_root = root.join("plugins");

    assert!(std::fs::create_dir_all(&repo_dir).is_ok());
    assert!(git_service::run_git(&repo_dir, &["init"]).is_ok());
    assert!(git_service::run_git(&repo_dir, &["config", "user.email", "dev@example.com"]).is_ok());
    assert!(git_service::run_git(&repo_dir, &["config", "user.name", "Dev User"]).is_ok());
    assert!(std::fs::write(repo_dir.join("README.md"), "base\n").is_ok());
    assert!(git_service::stage_paths(&repo_dir, &["README.md".to_string()]).is_ok());
    assert!(git_service::commit_create(&repo_dir, "base").is_ok());

    let mut child = Command::new(env!("CARGO_BIN_EXE_app_host"))
        .env("BRANCHFORGE_PLUGINS_ROOT", &plugins_root)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("spawn app_host");

    {
        let stdin = child.stdin.as_mut().expect("stdin");
        writeln!(stdin, "open {}", repo_dir.display()).expect("open");
        writeln!(stdin, "actions").expect("actions");
        writeln!(stdin, "run diagnostics.repo_capabilities").expect("run");
        writeln!(stdin, "show").expect("show");
        writeln!(stdin, "quit").expect("quit");
    }

    let output = child.wait_with_output().expect("wait");
    assert!(output.status.success());

    let stdout = String::from_utf8(output.stdout).expect("utf8 stdout");
    assert!(stdout.contains("Branchforge Console Runner"));
    assert!(stdout.contains("opened repository"));
    assert!(stdout.contains("Actions"));
    assert!(stdout.contains("diagnostics.repo_capabilities"));
    assert!(stdout.contains("[runner]"));
    assert!(stdout.contains("[window]"));
    assert!(stdout.contains("Diagnostics Panel"));
    assert!(stdout.contains("lfs_detected:"));
    assert!(stdout.contains("bye"));

    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn console_runner_supports_one_shot_command_mode() {
    let root = unique_temp_dir("one-shot");
    let plugins_root = root.join("plugins");
    let out_file = root.join("release_notes.md");

    assert!(std::fs::create_dir_all(&root).is_ok());

    let output = Command::new(env!("CARGO_BIN_EXE_app_host"))
        .env("BRANCHFORGE_PLUGINS_ROOT", &plugins_root)
        .args([
            "--command",
            &format!("run release.notes {} stable", out_file.to_string_lossy()),
        ])
        .output()
        .expect("run one-shot app_host");

    assert!(output.status.success());

    let stdout = String::from_utf8(output.stdout).expect("utf8 stdout");
    assert!(stdout.contains("release notes generated at"));
    assert!(!stdout.contains("bf> "));
    assert!(out_file.is_file());

    let rendered = std::fs::read_to_string(&out_file).unwrap_or_default();
    assert!(rendered.contains("Channel: stable"));

    let _ = std::fs::remove_dir_all(&root);
}
