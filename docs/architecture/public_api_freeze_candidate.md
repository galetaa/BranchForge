# Public API Freeze Candidate

This document lists the current public API surface and the remaining intentionally unstable areas after the v1.0 freeze pass.

## Public surface

- `plugin_sdk` re-exports from `plugin_api`
- RPC message types and codec (`RpcRequest`, `RpcResponse`, `RpcNotification`, `RpcMessage`, `FrameCodec`)
- Handshake contracts (`PluginHello`, `PluginRegister`, `HelloAck`)
- View and action specs (`ActionSpec`, `ViewSpec`, `DangerLevel`)
- Events (`RepoOpenedEvent`, `StateUpdatedEvent`, `JobFinishedEvent`)
- Preview/preflight types (`ActionPreflightRequest`, `ActionPreflightResult`, `ActionPreview`)

## Marked unstable

- Any future action with `DangerLevel::High` until it is documented in the compatibility matrix
- Experimental release/discovery extensions beyond the documented path/`file://`/`http://` registry source format

## Compatibility policy

- v1.0: breaking changes require major version bump
- alpha/beta releases may change without notice
- action ids, manifest v1, and RPC framing are stable for v1.0
- compatibility details are tracked in `docs/architecture/plugin_compatibility_matrix.md`

## Freeze checklist

- Audit plugin_api for internal-only structs
- Tag unstable APIs in docs
- Align SDK exports with intended public surface
- Define versioning policy for RPC methods
- Keep stabilization guidance in `docs/architecture/plugin_sdk_stabilization_guidelines.md`
