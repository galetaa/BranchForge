# Plugin SDK Stabilization Guidelines

## Stable by default in v1

- handshake contracts
- RPC envelope/frame types
- manifest v1 exports
- action/view spec types
- event payloads already documented in `plugin_api`

## Unstable or intentionally gated

- any API explicitly tagged as alpha/beta in docs

## Rules for future SDK changes

1. Additive fields are preferred over semantic rewrites.
2. Breaking manifest or protocol changes require a major version bump.
3. New unstable APIs must be documented in `public_api_freeze_candidate.md`.
4. Compatibility decisions must be mirrored in `plugin_compatibility_matrix.md`.

## Guidance for plugin authors

- Pin against the host major version you target.
- Rebuild external plugins when host major version changes.
- Treat documented action ids as stable within the host major version unless a doc explicitly marks them alpha/beta.
