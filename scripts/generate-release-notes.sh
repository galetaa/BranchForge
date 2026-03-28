#!/usr/bin/env bash
set -euo pipefail

repo_root="$(cd "$(dirname "$0")/.." && pwd)"
cd "$repo_root"

out_file="${1:-}"
if [[ "$out_file" == "" ]]; then
  echo "usage: $0 <out_file>" >&2
  exit 1
fi

channel="${BRANCHFORGE_RELEASE_CHANNEL:-local}"
cargo run -p app_host -- --command "run release.notes \"$out_file\" \"$channel\""
