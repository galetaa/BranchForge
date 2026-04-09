# Sample External Plugin

This is a minimal external plugin using the public alpha SDK.

## Build

```bash
cargo build --manifest-path external_plugins/sample_plugin/Cargo.toml
```

## Run Through The Host

```bash
cargo run -p app_host -- --command "plugin install external_plugins/sample_plugin"
cargo run -p app_host -- --command "actions"
cargo run -p app_host -- --command "run sample.ping"
```

`sample_external_plugin` is a framed stdio runtime process. If you launch the binary directly, it will wait for host handshake messages on stdin.

## Install (manual)

1. Build the plugin binary.
2. Package the binary together with `plugin.json`.
3. Install the package directory through the host plugin lifecycle flow.
4. Enable it from the diagnostics/plugin manager flow if needed.
5. Run the registered `sample.ping` action through the normal host action surface.
