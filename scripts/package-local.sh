#!/usr/bin/env bash
set -euo pipefail

repo_root="$(cd "$(dirname "$0")/.." && pwd)"
out_dir="$repo_root/target/tmp/local-package"

if [[ "${1:-}" != "" ]]; then
  out_dir="$1"
fi

# Build host + bundled plugins as release binaries for local package checks.
cd "$repo_root"
cargo build --release -p app_host -p repo_manager -p status -p history -p branches -p tags -p compare -p diagnostics

mkdir -p "$out_dir/bin" "$out_dir/plugins"
cp "$repo_root/target/release/app_host" "$out_dir/bin/"
cp "$repo_root/target/release/repo_manager" "$out_dir/plugins/"
cp "$repo_root/target/release/status" "$out_dir/plugins/"
cp "$repo_root/target/release/history" "$out_dir/plugins/"
cp "$repo_root/target/release/branches" "$out_dir/plugins/"
cp "$repo_root/target/release/tags" "$out_dir/plugins/"
cp "$repo_root/target/release/compare" "$out_dir/plugins/"
cp "$repo_root/target/release/diagnostics" "$out_dir/plugins/"

cat > "$out_dir/README.txt" <<EOF
Branchforge local package layout

bin/app_host          host executable
plugins/repo_manager  bundled plugin executable
plugins/status        bundled plugin executable
plugins/history       bundled plugin executable
plugins/branches      bundled plugin executable
plugins/tags          bundled plugin executable
plugins/compare       bundled plugin executable
plugins/diagnostics   bundled plugin executable

Run example:
  ./bin/app_host
EOF

sha="$(git rev-parse --short HEAD 2>/dev/null || echo local)"
date_utc="$(date -u '+%Y-%m-%d %H:%M')"
cat > "$out_dir/manifest.txt" <<EOF
commit=$sha
built_utc=$date_utc
layout=local-package-v1
EOF

echo "Local package created at: $out_dir"
