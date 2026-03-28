# Plugin Compatibility Matrix

## Stable v1 contracts

| Area | Version | Compatibility rule |
| --- | --- | --- |
| Manifest | `1` | breaking changes require a major host/plugin bump |
| Host protocol | `0.1` | plugin `protocol_version` must match exactly |
| RPC framing | v1 wire format | stable within host major version |
| Action/view specs | v1 host semantics | additive fields are allowed, breaking field meaning changes are not |

## Release channels

| Channel | Change policy |
| --- | --- |
| `stable` | no breaking contract changes inside the same major version |
| `beta` | feature flags and beta-only actions may change |
| `local` | packaging/dev channel for smoke verification |

## Discovery and install rules

| Surface | Rule |
| --- | --- |
| Direct install | package must contain `plugin.json` and `entrypoint` |
| Registry install | registry entry `plugin_id` must match package manifest `plugin_id` |
| Enable/disable/remove | scoped to local plugin storage only |

## Notes

- `rebase.interactive` is a stable v1 action id and follows the same compatibility guarantees as other documented action ids.
- Registry sources may be local filesystem paths, `file://...`, or `http://...` URLs. Remote registry entries may install packages through `manifest_url` + `entrypoint_url`.
