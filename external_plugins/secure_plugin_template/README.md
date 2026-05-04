# Secure Plugin Template

This template is the recommended starting point for out-of-tree plugins.

## Build

```sh
cargo build --manifest-path external_plugins/secure_plugin_template/Cargo.toml
```

Copy or symlink the built `secure_plugin_template` binary into this directory before installing it through BranchForge.

## Security Checklist

- Start with `read_state` only and add permissions one at a time.
- Keep Git execution in the host; plugins should request host actions instead of shelling out to Git.
- Include `plugin.sig` and `plugin.pub` beside `plugin.json` for cryptographically verified community distribution.
- Treat `write_repo`, `filesystem_write`, `network`, and `spawn_process` as high-impact permissions.
- Keep `protocol_version` aligned with `plugin_api::HOST_PLUGIN_PROTOCOL_VERSION`.

## Trust Levels

| Trust level | Requirement | Host behavior |
| --- | --- | --- |
| Bundled | Packaged with BranchForge | Trusted by distribution channel |
| SignedCommunity | Installed package contains `plugin.sig` and `plugin.pub` that verify `plugin.json` | Shown as signed community plugin |
| UnsignedLocal | Local package without signature | Shown with an unsigned-package warning |
| ExperimentalSandboxed | Installed but disabled unsigned package | Shown as sandboxed/experimental |
