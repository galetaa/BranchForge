# Sample External Plugin

This is a minimal external plugin using the public alpha SDK.

## Build

```bash
cargo build --manifest-path external_plugins/sample_plugin/Cargo.toml
```

## Run

```bash
./external_plugins/sample_plugin/target/debug/sample_external_plugin
```

## Install (manual)

1. Build the plugin binary.
2. Package the binary together with `plugin.json`.
3. Install the package directory through the host plugin lifecycle flow.
4. Enable it from the diagnostics/plugin manager flow if needed.
