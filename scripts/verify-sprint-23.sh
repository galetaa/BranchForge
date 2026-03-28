#!/usr/bin/env bash
set -euo pipefail

repo_root="$(cd "$(dirname "$0")/.." && pwd)"
cd "$repo_root"

package_out="$repo_root/target/tmp/sprint23-package-check"
cargo run -p app_host -- --command "run verify.sprint23 \"$package_out\""
