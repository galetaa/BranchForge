#!/usr/bin/env bash
set -euo pipefail

repo_root="$(cd "$(dirname "$0")/.." && pwd)"
cd "$repo_root"

artifact_dir="${1:-}"

if [[ "$artifact_dir" == "" ]]; then
  echo "usage: $0 <artifact_dir>" >&2
  exit 1
fi

cargo run -p app_host -- --command "run release.sign \"$artifact_dir\""
