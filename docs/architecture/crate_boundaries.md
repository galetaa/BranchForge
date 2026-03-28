# Crate boundaries and dependency directions

## Ownership

- `app_host`: app entrypoint and host wiring only
- `plugin_api`: shared protocol models for host/plugins
- `plugin_host`: plugin process lifecycle and rpc transport integration
- `action_engine`: action routing between UI and plugin host
- `state_store`: snapshots and selection state updates
- `job_system`: job queue, lock policy, result events
- `git_service`: the only place allowed to call git CLI
- `ui_shell`: host-rendered viewmodel components
- `plugins/repo_manager`: repo.open flow implementation
- `plugins/status`: status/stage/unstage/commit flow implementation

## Allowed dependency directions

- `app_host` -> all host-side crates
- `plugin_host` -> `plugin_api`
- `action_engine` -> `plugin_host`, `state_store`, `plugin_api`
- `job_system` -> `git_service`, `state_store`, `plugin_api`
- `ui_shell` -> `state_store`, `plugin_api`
- `state_store` -> `plugin_api`
- `git_service` -> no workspace runtime dependencies
- `plugins/*` -> `plugin_api` only

## Explicit restrictions

- Plugin crates must not depend on `git_service`.
- Git CLI execution is forbidden outside `git_service`.
- Cyclic dependencies between workspace crates are not allowed.
- `ops.check_deps` is the required automated guard for plugin dependency rules; `scripts/check-deps.sh` is a compatibility shim over the runtime command.

