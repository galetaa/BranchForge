# External Plugin Developer Guide

## Scope

This guide covers the supported v1 external plugin flow:

1. build a plugin against `plugin_sdk`
2. package it with `plugin.json`
3. validate compatibility locally
4. install directly or through the local registry index
5. optionally publish the registry index over `file://` or `http://`

## Package contract

An installable package directory must contain:

- `plugin.json`
- the executable referenced by `entrypoint`

`plugin.json` must use manifest v1 and protocol `0.1`.

## Recommended development loop

```bash
cargo build --manifest-path external_plugins/sample_plugin/Cargo.toml
plugin discover plugin_registry
plugin install-registry sample_external plugin_registry
```

Direct install remains available:

```bash
plugin install external_plugins/sample_plugin
```

## Safety metadata

Expose accurate `ActionEffects` and `ConfirmPolicy` values for every action:

- mutating index/worktree actions should set `writes_index` / `writes_worktree`
- destructive actions should use `confirm_policy = always`
- networked actions should set `network = true`

## Compatibility

- Stable surface: manifest v1, RPC framing, handshake types, action/view specs
- Versioning rules: see `docs/architecture/plugin_compatibility_matrix.md`
- Stabilization rules: see `docs/architecture/plugin_sdk_stabilization_guidelines.md`

## Registry discovery

Discovery reads `plugin_registry/registry.json` by default. Registry sources may be a local path, `file://...`, or `http://...`. Each entry is validated against the package manifest before installation.

Example entry:

```json
{
  "plugin_id": "sample_external",
  "package_dir": "../external_plugins/sample_plugin",
  "summary": "Sample external plugin package",
  "channel": "stable"
}
```

Remote entry example:

```json
{
  "plugin_id": "sample_external",
  "manifest_url": "plugin.json",
  "entrypoint_url": "sample_external_plugin",
  "summary": "Sample external plugin package",
  "channel": "stable"
}
```

## Verification checklist

- `cargo test -p plugin_host`
- `cargo test -p app_host --test sprint22_plugin_extensibility_smoke`
- `plugin discover <registry_path>`
- `plugin install-registry <plugin_id> <registry_path>`
