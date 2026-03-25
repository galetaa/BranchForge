# Public Plugin SDK (alpha)

This document defines the public subset of the plugin API for alpha external plugins.

## Manifest rules (v1)

External plugins must ship a `plugin.json` manifest next to the binary:

```json
{
  "manifest_version": "1",
  "plugin_id": "sample_external",
  "version": "0.1.0",
  "protocol_version": "0.1",
  "entrypoint": "sample_external_plugin",
  "description": "Sample external plugin",
  "permissions": ["read_state"]
}
```

- `manifest_version` must be `1`.
- `plugin_id` must be unique inside local plugin storage.
- `entrypoint` is a relative path inside the package directory.
- `protocol_version` must match host protocol (`0.1` in this release).

## Public API surface (alpha)

The SDK re-exports the host contract from `plugin_api`, including:

- handshake types: `PluginHello`, `HelloAck`, `PluginRegister`
- messaging: `RpcRequest`, `RpcResponse`, `RpcNotification`, `RpcMessage`
- helpers: `FrameCodec`
- manifest: `PluginManifestV1`, `PLUGIN_MANIFEST_VERSION_V1`, `HOST_PLUGIN_PROTOCOL_VERSION`
- view/action specs: `ActionSpec`, `ActionEffects`, `ConfirmPolicy`, `ViewSpec`, `DangerLevel`
- events: `RepoOpenedEvent`, `StateUpdatedEvent`, `JobFinishedEvent`

See `crates/plugin_sdk/src/lib.rs` for the exact export list.

## Action metadata (Sprint 15 baseline)

`ActionSpec` now includes baseline safety metadata used by host-side confirmation flow:

- `effects` (`ActionEffects`): `writes_refs`, `writes_index`, `writes_worktree`, `network`, `danger_level`
- `confirm_policy` (`ConfirmPolicy`): `never`, `on_danger`, `always`

Recommendations for plugin authors:

1. Mark all mutating actions with accurate `effects` values.
2. Use `confirm_policy = always` for destructive operations (for example delete/discard).
3. Keep `danger` aligned with `effects.danger_level` during alpha to simplify operator review.

## Package format

A local plugin package is a directory containing at least:

1. `plugin.json`
2. plugin executable from `entrypoint`

Host-side install pipeline validates:

- manifest schema,
- protocol compatibility,
- existence of entrypoint binary.

If compatibility fails, installation is rejected with explicit reason.

## Version compatibility

- Host protocol version: `0.1`
- A plugin with another `protocol_version` is rejected as incompatible.
- Public SDK evolves under staged freeze; rebuild plugin when upgrading host major milestones.

## Manual install flow

1. Build the plugin binary.
2. Create `plugin.json` in the same directory.
3. Install local package through host plugin manager flow.
4. Enable/disable/remove plugin from local manager.

Refer to `external_plugins/sample_plugin/README.md` for a working sample.
