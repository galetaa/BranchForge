# Public Plugin SDK (alpha)

This document defines the public subset of the plugin API for alpha external plugins.

## Manifest rules

External plugins should include a minimal `plugin.toml` next to the binary:

```toml
id = "sample_external"
name = "Sample External Plugin"
version = "0.1.0"
binary = "sample_external_plugin"
entry = "sample_external_plugin"
```

- `id` must be unique.
- `binary` and `entry` point to the executable file.
- The host is responsible for discovering and spawning the binary.

## Public API surface (alpha)

The alpha SDK re-exports a stable subset from `plugin_api`:

- handshake types: `PluginHello`, `HelloAck`, `PluginRegister`
- messaging: `RpcRequest`, `RpcResponse`, `RpcNotification`, `RpcMessage`
- helpers: `FrameCodec`
- view/action specs: `ActionSpec`, `ViewSpec`, `DangerLevel`
- events: `RepoOpenedEvent`, `StateUpdatedEvent`, `JobFinishedEvent`

See `crates/plugin_sdk/src/lib.rs` for the exact export list.

## Version compatibility

- Protocol version is `0.1`.
- Alpha plugins should be rebuilt when the SDK changes.
- There is no stable API promise in v0.5/v0.6.

## Manual install flow

1. Build the plugin binary.
2. Keep `plugin.toml` next to the binary.
3. Configure the host to spawn the binary (alpha: manual wiring).

Refer to `external_plugins/sample_plugin/README.md` for a working sample.
