# Public API Freeze Candidate

This document lists the current public API surface and marks unstable areas for v1.0 planning.

## Public surface (candidate)

- `plugin_sdk` re-exports from `plugin_api`
- RPC message types and codec (`RpcRequest`, `RpcResponse`, `RpcNotification`, `RpcMessage`, `FrameCodec`)
- Handshake contracts (`PluginHello`, `PluginRegister`, `HelloAck`)
- View and action specs (`ActionSpec`, `ViewSpec`, `DangerLevel`)
- Events (`RepoOpenedEvent`, `StateUpdatedEvent`, `JobFinishedEvent`)
- Preview/preflight types (`ActionPreflightRequest`, `ActionPreflightResult`, `ActionPreview`)

## Marked unstable

- Beta-only action IDs (`rebase.interactive`)
- Any new action with `DangerLevel::High` pending confirm UX improvements
- SDK policy for external plugin discovery/manifest

## Compatibility policy (draft)

- v1.0: breaking changes require major version bump
- alpha/beta releases may change without notice
- public API freeze checklist to be completed before v1.0

## Freeze checklist (draft)

- Audit plugin_api for internal-only structs
- Tag unstable APIs in docs
- Align SDK exports with intended public surface
- Define versioning policy for RPC methods
