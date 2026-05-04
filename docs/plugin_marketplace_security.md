# Plugin Marketplace Security

BranchForge treats external plugins as process-isolated extensions with explicit risk metadata.

## Trust States

- Bundled plugins ship with the application and are trusted as part of the release.
- Signed community plugins must include `plugin.sig` and `plugin.pub`; BranchForge verifies `plugin.json` with `openssl dgst -sha256 -verify`.
- Unsigned local plugins are allowed only with visible warnings.
- Future sandboxed plugins are expected to use a WASM capability model.

## Marketplace Flow

- `plugin.marketplace [registry_path]` discovers catalog entries, compares installed versions, and marks update availability.
- `plugin.install_registry <plugin_id> [registry_path]` installs a catalog plugin after manifest compatibility checks.
- `plugin.update <plugin_id> [registry_path]` replaces the installed plugin with the catalog version and refreshes diagnostics.

## Sandbox Modes

Current plugins run out-of-process. BranchForge classifies risk by permissions:

- `process-isolated-read-mostly`: no high-impact permissions.
- `process-isolated-permission-gated`: network or repository write permissions.
- `process-isolated-high-impact`: process spawning or filesystem write permissions.

Plugins never receive host credentials unless a future permission explicitly grants that capability.
