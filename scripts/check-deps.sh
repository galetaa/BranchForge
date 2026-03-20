#!/usr/bin/env zsh
set -euo pipefail

workspace_root="$(cd "$(dirname "$0")/.." && pwd)"

# T05 guard: bundled plugins must not depend on git_service directly.
for manifest in "$workspace_root"/plugins/*/Cargo.toml; do
  if grep -Eq '^\s*git_service\s*=' "$manifest"; then
    echo "dependency guard failed: plugin manifest depends on git_service: $manifest" >&2
    exit 1
  fi
done

echo "dependency guards passed"

