use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::{SystemTime, UNIX_EPOCH};

use sha2::{Digest, Sha256};

const BUNDLED_BINARIES: &[(&str, &str)] = &[
    ("app_host", "bin/app_host"),
    ("repo_manager", "plugins/repo_manager"),
    ("status", "plugins/status"),
    ("history", "plugins/history"),
    ("branches", "plugins/branches"),
    ("tags", "plugins/tags"),
    ("compare", "plugins/compare"),
    ("diagnostics", "plugins/diagnostics"),
];

const RELEASE_NOTES_DOC: &str = "docs/process/release_notes_v1.0.1.md";
const CHANGELOG_DOC: &str = "docs/process/changelog_v1.0.1.md";
const SUPPORT_DOC: &str = "docs/process/known_issues_and_support_v1.0.1.md";
const REGRESSION_DOC: &str = "docs/process/release_regression_matrix_sprint24.md";
const RC_SIGNOFF_DOC: &str = "docs/process/rc_signoff_sprint24.md";

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LocalPackageOptions {
    pub out_dir: PathBuf,
    pub channel: String,
    pub rollback_from: String,
}

impl Default for LocalPackageOptions {
    fn default() -> Self {
        Self {
            out_dir: workspace_root().join("target/tmp/local-package"),
            channel: "local".to_string(),
            rollback_from: "last-stable".to_string(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReleasePackageOptions {
    pub out_dir: PathBuf,
    pub channel: String,
    pub rollback_from: String,
}

impl Default for ReleasePackageOptions {
    fn default() -> Self {
        Self {
            out_dir: workspace_root().join("target/tmp/release-package"),
            channel: "stable".to_string(),
            rollback_from: "last-stable".to_string(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PackageReleaseResult {
    pub out_dir: PathBuf,
    pub archive_path: PathBuf,
}

pub fn workspace_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(Path::parent)
        .unwrap_or_else(|| Path::new(env!("CARGO_MANIFEST_DIR")))
        .to_path_buf()
}

pub fn check_dependency_guards(repo_root: &Path) -> Result<String, String> {
    let plugins_root = repo_root.join("plugins");
    let entries = fs::read_dir(&plugins_root)
        .map_err(|err| format!("{}: {}", plugins_root.display(), err))?;
    for entry in entries {
        let entry = entry.map_err(|err| err.to_string())?;
        let manifest = entry.path().join("Cargo.toml");
        if !manifest.exists() {
            continue;
        }
        let raw = fs::read_to_string(&manifest)
            .map_err(|err| format!("{}: {}", manifest.display(), err))?;
        if raw.lines().any(|line| {
            let line = line.trim_start();
            line.starts_with("git_service") && line.contains('=')
        }) {
            return Err(format!(
                "dependency guard failed: plugin manifest depends on git_service: {}",
                manifest.display()
            ));
        }
    }
    Ok("dependency guards passed".to_string())
}

pub fn run_dev_check(repo_root: &Path) -> Result<String, String> {
    check_dependency_guards(repo_root)?;
    run_command(repo_root, "cargo", &["fmt", "--all", "--check"], &[])?;
    run_command(
        repo_root,
        "cargo",
        &[
            "clippy",
            "--workspace",
            "--all-targets",
            "--all-features",
            "--",
            "-D",
            "warnings",
        ],
        &[],
    )?;
    run_command(repo_root, "cargo", &["test", "--workspace"], &[])?;
    Ok("dev check passed".to_string())
}

pub fn generate_release_notes(
    repo_root: &Path,
    out_file: &Path,
    channel: &str,
) -> Result<String, String> {
    let version = env!("CARGO_PKG_VERSION");
    let generated_utc = current_utc_string(repo_root);
    let release_notes = strip_first_heading(&read_file(repo_root.join(RELEASE_NOTES_DOC))?);
    let changelog = strip_first_heading(&read_file(repo_root.join(CHANGELOG_DOC))?);
    let support = strip_first_heading(&read_file(repo_root.join(SUPPORT_DOC))?);

    let content = format!(
        "# Branchforge {version} Release Notes\n\n- Channel: {channel}\n- Generated UTC: {generated_utc}\n- Source docs:\n  - {RELEASE_NOTES_DOC}\n  - {CHANGELOG_DOC}\n  - {SUPPORT_DOC}\n\n## Product Notes\n\n{release_notes}\n\n## Changelog\n\n{changelog}\n\n## Support\n\n{support}\n"
    );
    if let Some(parent) = out_file.parent() {
        fs::create_dir_all(parent).map_err(|err| format!("{}: {}", parent.display(), err))?;
    }
    fs::write(out_file, content).map_err(|err| format!("{}: {}", out_file.display(), err))?;
    Ok(format!("release notes generated at {}", out_file.display()))
}

pub fn sign_artifacts(artifact_dir: &Path) -> Result<String, String> {
    let checksums = artifact_dir.join("sha256sums.txt");
    if !checksums.is_file() {
        return Err(format!(
            "missing checksum manifest: {}",
            checksums.display()
        ));
    }

    let signature = artifact_dir.join("sha256sums.sig");
    let public_key = artifact_dir.join("sha256sums.pub");
    let generated_utc = current_utc_string(&workspace_root());

    let (private_key, mode, cleanup_private_key) =
        if let Ok(configured) = std::env::var("BRANCHFORGE_SIGNING_KEY") {
            (PathBuf::from(configured), "configured".to_string(), false)
        } else {
            let temp_key = artifact_dir.join(format!("tmp-signing-key-{}.pem", unique_suffix()));
            run_command(
                artifact_dir,
                "openssl",
                &[
                    "genpkey",
                    "-algorithm",
                    "RSA",
                    "-pkeyopt",
                    "rsa_keygen_bits:2048",
                    "-out",
                    &temp_key.display().to_string(),
                ],
                &[],
            )?;
            (temp_key, "ephemeral-dev".to_string(), true)
        };

    let result = (|| -> Result<(), String> {
        run_command(
            artifact_dir,
            "openssl",
            &[
                "rsa",
                "-pubout",
                "-in",
                &private_key.display().to_string(),
                "-out",
                &public_key.display().to_string(),
            ],
            &[],
        )?;
        run_command(
            artifact_dir,
            "openssl",
            &[
                "dgst",
                "-sha256",
                "-sign",
                &private_key.display().to_string(),
                "-out",
                &signature.display().to_string(),
                &checksums.display().to_string(),
            ],
            &[],
        )?;
        fs::write(
            artifact_dir.join("signing.json"),
            format!(
                "{{\n  \"signed\": true,\n  \"mode\": \"{mode}\",\n  \"algorithm\": \"sha256+rsa\",\n  \"generated_utc\": \"{generated_utc}\",\n  \"signature_file\": \"sha256sums.sig\",\n  \"public_key_file\": \"sha256sums.pub\"\n}}\n"
            ),
        )
        .map_err(|err| format!("{}: {}", artifact_dir.display(), err))?;
        Ok(())
    })();

    if cleanup_private_key {
        let _ = fs::remove_file(&private_key);
    }
    result?;

    Ok(format!(
        "artifacts signed in {} ({mode})",
        artifact_dir.display()
    ))
}

pub fn package_local(repo_root: &Path, options: &LocalPackageOptions) -> Result<String, String> {
    run_command(
        repo_root,
        "cargo",
        &[
            "build",
            "--release",
            "-p",
            "app_host",
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
        ],
        &[],
    )?;

    fs::create_dir_all(options.out_dir.join("bin"))
        .map_err(|err| format!("{}: {}", options.out_dir.display(), err))?;
    fs::create_dir_all(options.out_dir.join("plugins"))
        .map_err(|err| format!("{}: {}", options.out_dir.display(), err))?;

    let release_root = repo_root.join("target/release");
    for (binary, relative_path) in BUNDLED_BINARIES {
        let source = release_root.join(binary);
        let target = options.out_dir.join(relative_path);
        if let Some(parent) = target.parent() {
            fs::create_dir_all(parent).map_err(|err| format!("{}: {}", parent.display(), err))?;
        }
        fs::copy(&source, &target)
            .map_err(|err| format!("{} -> {}: {}", source.display(), target.display(), err))?;
    }

    fs::write(
        options.out_dir.join("README.txt"),
        "Branchforge local package layout\n\nbin/app_host          host executable\nplugins/repo_manager  bundled plugin executable\nplugins/status        bundled plugin executable\nplugins/history       bundled plugin executable\nplugins/branches      bundled plugin executable\nplugins/tags          bundled plugin executable\nplugins/compare       bundled plugin executable\nplugins/diagnostics   bundled plugin executable\n\nRun example:\n  ./bin/app_host\n",
    )
    .map_err(|err| format!("{}: {}", options.out_dir.display(), err))?;

    let sha = short_commit_sha(repo_root);
    let date_utc = current_utc_string(repo_root);
    let platform = platform_label();
    let version = env!("CARGO_PKG_VERSION");

    fs::write(
        options.out_dir.join("manifest.txt"),
        format!(
            "version={version}\nchannel={}\ncommit={sha}\nbuilt_utc={date_utc}\nplatform={platform}\nlayout=local-package-v1\n",
            options.channel
        ),
    )
    .map_err(|err| format!("{}: {}", options.out_dir.display(), err))?;

    fs::write(
        options.out_dir.join("manifest.json"),
        format!(
            "{{\n  \"version\": \"{version}\",\n  \"channel\": \"{}\",\n  \"commit\": \"{sha}\",\n  \"built_utc\": \"{date_utc}\",\n  \"platform\": \"{platform}\",\n  \"layout\": \"local-package-v1\",\n  \"rollback_from\": \"{}\",\n  \"protocol_version\": \"0.1\"\n}}\n",
            options.channel, options.rollback_from
        ),
    )
    .map_err(|err| format!("{}: {}", options.out_dir.display(), err))?;

    fs::write(
        options.out_dir.join("rollback.json"),
        format!(
            "{{\n  \"channel\": \"{}\",\n  \"rollback_from\": \"{}\",\n  \"rollback_target\": \"last-stable-build\",\n  \"reversible_migrations\": true\n}}\n",
            options.channel, options.rollback_from
        ),
    )
    .map_err(|err| format!("{}: {}", options.out_dir.display(), err))?;

    generate_release_notes(
        repo_root,
        &options.out_dir.join("release_notes.md"),
        &options.channel,
    )?;
    fs::copy(
        repo_root.join(CHANGELOG_DOC),
        options.out_dir.join("changelog.md"),
    )
    .map_err(|err| format!("{}: {}", options.out_dir.display(), err))?;
    fs::copy(
        repo_root.join(SUPPORT_DOC),
        options.out_dir.join("support.md"),
    )
    .map_err(|err| format!("{}: {}", options.out_dir.display(), err))?;

    let checksum_files = BUNDLED_BINARIES
        .iter()
        .map(|(_, relative)| relative.to_string())
        .chain(
            [
                "manifest.txt",
                "manifest.json",
                "rollback.json",
                "release_notes.md",
                "changelog.md",
                "support.md",
                "README.txt",
            ]
            .into_iter()
            .map(str::to_string),
        )
        .collect::<Vec<_>>();
    write_checksum_manifest(&options.out_dir, &checksum_files)?;
    sign_artifacts(&options.out_dir)?;

    Ok(format!(
        "local package created at {}\nchannel={}\nrollback_from={}",
        options.out_dir.display(),
        options.channel,
        options.rollback_from
    ))
}

pub fn package_release(
    repo_root: &Path,
    options: &ReleasePackageOptions,
) -> Result<PackageReleaseResult, String> {
    package_local(
        repo_root,
        &LocalPackageOptions {
            out_dir: options.out_dir.clone(),
            channel: options.channel.clone(),
            rollback_from: options.rollback_from.clone(),
        },
    )?;

    let archive_dir = options
        .out_dir
        .parent()
        .ok_or_else(|| {
            format!(
                "invalid release package output: {}",
                options.out_dir.display()
            )
        })?
        .to_path_buf();
    let archive_name = format!(
        "branchforge-{}-{}-{}.tar.gz",
        env!("CARGO_PKG_VERSION"),
        options.channel,
        platform_label()
    );
    let archive_path = archive_dir.join(&archive_name);
    run_command(
        &archive_dir,
        "tar",
        &[
            "-czf",
            &archive_path.display().to_string(),
            "-C",
            &archive_dir.display().to_string(),
            options
                .out_dir
                .file_name()
                .and_then(|name| name.to_str())
                .ok_or_else(|| "invalid output dir name".to_string())?,
        ],
        &[],
    )?;
    let archive_hash = hash_file(&archive_path)?;
    fs::write(
        archive_dir.join(format!("{archive_name}.sha256")),
        format!("{archive_hash}  {archive_name}\n"),
    )
    .map_err(|err| format!("{}: {}", archive_dir.display(), err))?;

    Ok(PackageReleaseResult {
        out_dir: options.out_dir.clone(),
        archive_path,
    })
}

pub fn verify_release(repo_root: &Path, options: &ReleasePackageOptions) -> Result<String, String> {
    verify_sprint24(repo_root, options)
}

pub fn verify_sprint22(repo_root: &Path) -> Result<String, String> {
    check_dependency_guards(repo_root)?;
    run_command(
        repo_root,
        "cargo",
        &["test", "-p", "plugin_sdk", "--", "--nocapture"],
        &[],
    )?;
    run_command(
        repo_root,
        "cargo",
        &["test", "-p", "plugin_host", "--", "--nocapture"],
        &[],
    )?;
    run_command(
        repo_root,
        "cargo",
        &[
            "test",
            "-p",
            "app_host",
            "--test",
            "sprint22_plugin_extensibility_smoke",
            "--",
            "--nocapture",
        ],
        &[],
    )?;
    Ok("Sprint 22 local verification passed".to_string())
}

pub fn verify_sprint23(repo_root: &Path, out_dir: &Path) -> Result<String, String> {
    check_dependency_guards(repo_root)?;
    run_command(
        repo_root,
        "cargo",
        &["test", "-p", "state_store", "--", "--nocapture"],
        &[],
    )?;
    run_command(
        repo_root,
        "cargo",
        &["test", "-p", "ui_shell", "--", "--nocapture"],
        &[],
    )?;
    run_command(
        repo_root,
        "cargo",
        &[
            "test",
            "-p",
            "app_host",
            "--test",
            "sprint23_beta_hardening_smoke",
            "--",
            "--nocapture",
        ],
        &[],
    )?;

    package_local(
        repo_root,
        &LocalPackageOptions {
            out_dir: out_dir.to_path_buf(),
            ..LocalPackageOptions::default()
        },
    )?;
    for required in [out_dir.join("manifest.txt"), out_dir.join("bin/app_host")] {
        if !required.exists() {
            return Err(format!("required artifact missing: {}", required.display()));
        }
    }

    Ok(format!(
        "Sprint 23 local verification passed\npackage={}",
        out_dir.display()
    ))
}

pub fn verify_sprint24(
    repo_root: &Path,
    options: &ReleasePackageOptions,
) -> Result<String, String> {
    check_dependency_guards(repo_root)?;
    verify_sprint22(repo_root)?;
    let sprint23_out = options
        .out_dir
        .parent()
        .unwrap_or(repo_root)
        .join("sprint23-package-check");
    verify_sprint23(repo_root, &sprint23_out)?;

    let release = package_release(repo_root, options)?;
    verify_release_artifacts(repo_root, &release)?;

    Ok(format!(
        "Sprint 24 local verification passed\npackage={}\narchive={}",
        release.out_dir.display(),
        release.archive_path.display()
    ))
}

fn verify_release_artifacts(
    repo_root: &Path,
    release: &PackageReleaseResult,
) -> Result<(), String> {
    let checksums = release.out_dir.join("sha256sums.txt");
    let signature = release.out_dir.join("sha256sums.sig");
    let public_key = release.out_dir.join("sha256sums.pub");
    let required = [
        release.out_dir.join("manifest.txt"),
        release.out_dir.join("manifest.json"),
        checksums.clone(),
        release.out_dir.join("signing.json"),
        release.out_dir.join("rollback.json"),
        release.out_dir.join("release_notes.md"),
        repo_root.join(RELEASE_NOTES_DOC),
        repo_root.join(CHANGELOG_DOC),
        repo_root.join(SUPPORT_DOC),
        repo_root.join(REGRESSION_DOC),
        repo_root.join(RC_SIGNOFF_DOC),
        release
            .archive_path
            .parent()
            .unwrap_or(repo_root)
            .join(format!(
                "{}.sha256",
                release
                    .archive_path
                    .file_name()
                    .and_then(|name| name.to_str())
                    .unwrap_or("release.tar.gz")
            )),
    ];
    for path in required {
        if !path.exists() {
            return Err(format!("required artifact missing: {}", path.display()));
        }
    }

    run_command(
        repo_root,
        "openssl",
        &[
            "dgst",
            "-sha256",
            "-verify",
            &public_key.display().to_string(),
            "-signature",
            &signature.display().to_string(),
            &checksums.display().to_string(),
        ],
        &[],
    )?;
    Ok(())
}

fn write_checksum_manifest(out_dir: &Path, relative_files: &[String]) -> Result<(), String> {
    let mut lines = Vec::new();
    for relative in relative_files {
        let path = out_dir.join(relative);
        let hash = hash_file(&path)?;
        lines.push(format!("{hash}  {relative}"));
    }
    fs::write(
        out_dir.join("sha256sums.txt"),
        format!("{}\n", lines.join("\n")),
    )
    .map_err(|err| format!("{}: {}", out_dir.display(), err))
}

fn hash_file(path: &Path) -> Result<String, String> {
    let data = fs::read(path).map_err(|err| format!("{}: {}", path.display(), err))?;
    let mut hasher = Sha256::new();
    hasher.update(data);
    let digest = hasher.finalize();
    Ok(digest.iter().map(|byte| format!("{byte:02x}")).collect())
}

fn read_file(path: PathBuf) -> Result<String, String> {
    fs::read_to_string(&path).map_err(|err| format!("{}: {}", path.display(), err))
}

fn strip_first_heading(content: &str) -> String {
    content
        .split_once('\n')
        .map(|(_, rest)| rest.trim_start_matches('\n').to_string())
        .unwrap_or_default()
}

fn short_commit_sha(repo_root: &Path) -> String {
    run_command(repo_root, "git", &["rev-parse", "--short", "HEAD"], &[])
        .map(|value| value.lines().next().unwrap_or("local").trim().to_string())
        .unwrap_or_else(|_| "local".to_string())
}

fn current_utc_string(repo_root: &Path) -> String {
    run_command(repo_root, "date", &["-u", "+%Y-%m-%d %H:%M"], &[])
        .map(|value| value.lines().next().unwrap_or_default().trim().to_string())
        .unwrap_or_else(|_| {
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .map(|duration| duration.as_secs().to_string())
                .unwrap_or_else(|_| "0".to_string())
        })
}

fn platform_label() -> String {
    format!("{}-{}", std::env::consts::OS, std::env::consts::ARCH)
}

fn unique_suffix() -> u128 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_nanos())
        .unwrap_or(0)
}

fn run_command(
    cwd: &Path,
    program: &str,
    args: &[&str],
    envs: &[(&str, &str)],
) -> Result<String, String> {
    let mut command = Command::new(program);
    command.current_dir(cwd);
    command.args(args);
    for (key, value) in envs {
        command.env(key, value);
    }
    let output = command.output().map_err(|err| {
        format!(
            "failed to start `{}` in {}: {}",
            program,
            cwd.display(),
            err
        )
    })?;
    if output.status.success() {
        let stdout = String::from_utf8_lossy(&output.stdout).trim().to_string();
        if stdout.is_empty() {
            Ok(String::from_utf8_lossy(&output.stderr).trim().to_string())
        } else {
            Ok(stdout)
        }
    } else {
        let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
        let stdout = String::from_utf8_lossy(&output.stdout).trim().to_string();
        let detail = if !stderr.is_empty() { stderr } else { stdout };
        Err(format!(
            "`{} {}` failed: {}",
            program,
            args.join(" "),
            detail
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_root(label: &str) -> PathBuf {
        let nanos = unique_suffix();
        std::env::temp_dir().join(format!("branchforge-ops-{label}-{nanos}"))
    }

    #[test]
    fn dependency_guards_reject_git_service_in_plugin_manifest() {
        let root = temp_root("deps");
        let plugin_dir = root.join("plugins/bad");
        assert!(fs::create_dir_all(&plugin_dir).is_ok());
        assert!(
            fs::write(
                plugin_dir.join("Cargo.toml"),
                "[dependencies]\ngit_service = { path = \"../git_service\" }\n",
            )
            .is_ok()
        );

        let result = check_dependency_guards(&root);
        assert!(result.is_err());

        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn generate_release_notes_combines_docs() {
        let root = temp_root("notes");
        assert!(fs::create_dir_all(root.join("docs/process")).is_ok());
        assert!(fs::write(root.join(RELEASE_NOTES_DOC), "# Notes\nrelease body\n").is_ok());
        assert!(fs::write(root.join(CHANGELOG_DOC), "# Changelog\nchange body\n").is_ok());
        assert!(fs::write(root.join(SUPPORT_DOC), "# Support\nsupport body\n").is_ok());

        let out_file = root.join("out/release_notes.md");
        let result = generate_release_notes(&root, &out_file, "stable");
        assert!(result.is_ok());

        let rendered = fs::read_to_string(&out_file).unwrap_or_default();
        assert!(rendered.contains("Channel: stable"));
        assert!(rendered.contains("release body"));
        assert!(rendered.contains("change body"));
        assert!(rendered.contains("support body"));

        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn checksum_manifest_uses_sha256_format() {
        let root = temp_root("checksums");
        assert!(fs::create_dir_all(&root).is_ok());
        assert!(fs::write(root.join("demo.txt"), "demo").is_ok());

        let result = write_checksum_manifest(&root, &["demo.txt".to_string()]);
        assert!(result.is_ok());
        let manifest = fs::read_to_string(root.join("sha256sums.txt")).unwrap_or_default();
        assert!(manifest.contains("demo.txt"));
        assert_eq!(manifest.lines().count(), 1);

        let _ = fs::remove_dir_all(root);
    }
}
