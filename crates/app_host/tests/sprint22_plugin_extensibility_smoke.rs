#[test]
fn sprint22_local_plugin_lifecycle_and_compatibility() {
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0);
    let root = std::env::temp_dir().join(format!("branchforge-sprint22-smoke-{nanos}"));
    let package_ok = root.join("pkg-ok");
    let package_bad = root.join("pkg-bad");
    let plugins_root = root.join("installed");

    assert!(std::fs::create_dir_all(&package_ok).is_ok());
    assert!(std::fs::create_dir_all(&package_bad).is_ok());

    assert!(std::fs::write(package_ok.join("plugin_bin"), "#!/usr/bin/env sh\nexit 0\n").is_ok());
    assert!(
        std::fs::write(
            package_bad.join("plugin_bin"),
            "#!/usr/bin/env sh\nexit 0\n"
        )
        .is_ok()
    );

    let ok_manifest = plugin_api::PluginManifestV1 {
        manifest_version: plugin_api::PLUGIN_MANIFEST_VERSION_V1.to_string(),
        plugin_id: "sample_ok".to_string(),
        version: "0.1.0".to_string(),
        protocol_version: plugin_api::HOST_PLUGIN_PROTOCOL_VERSION.to_string(),
        entrypoint: "plugin_bin".to_string(),
        description: Some("ok plugin".to_string()),
        permissions: vec!["read_state".to_string()],
    };
    let bad_manifest = plugin_api::PluginManifestV1 {
        manifest_version: plugin_api::PLUGIN_MANIFEST_VERSION_V1.to_string(),
        plugin_id: "sample_bad".to_string(),
        version: "0.1.0".to_string(),
        protocol_version: "9.9".to_string(),
        entrypoint: "plugin_bin".to_string(),
        description: None,
        permissions: Vec::new(),
    };

    let ok_raw = serde_json::to_string_pretty(&ok_manifest).unwrap_or_else(|_| "{}".to_string());
    let bad_raw = serde_json::to_string_pretty(&bad_manifest).unwrap_or_else(|_| "{}".to_string());
    assert!(std::fs::write(package_ok.join("plugin.json"), ok_raw).is_ok());
    assert!(std::fs::write(package_bad.join("plugin.json"), bad_raw).is_ok());

    let installed = plugin_host::install_local_plugin(&package_ok, &plugins_root);
    assert!(installed.is_ok());

    let listed = plugin_host::list_installed_plugins(&plugins_root).expect("list");
    assert_eq!(listed.len(), 1);
    assert_eq!(listed[0].manifest.plugin_id, "sample_ok");

    let disabled = plugin_host::set_plugin_enabled(&plugins_root, "sample_ok", false);
    assert!(disabled.is_ok());
    assert!(!disabled.expect("disable").enabled);

    let incompatible = plugin_host::install_local_plugin(&package_bad, &plugins_root);
    assert!(matches!(
        incompatible,
        Err(plugin_host::PluginManagerError::IncompatiblePlugin { .. })
    ));

    assert!(plugin_host::remove_local_plugin(&plugins_root, "sample_ok").is_ok());
    let listed = plugin_host::list_installed_plugins(&plugins_root).expect("list");
    assert!(listed.is_empty());

    let _ = std::fs::remove_dir_all(root);
}
