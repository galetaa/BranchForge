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
2. Copy it into a folder on your PATH or alongside the host binary.
3. Configure your host runner to spawn `sample_external_plugin` (see `docs/architecture/plugin_sdk_alpha.md`).
