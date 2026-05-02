#!/usr/bin/env bash
set -euo pipefail

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
out_dir="${1:-$repo_root/target/tmp/desktop-alpha-verify}"

"$repo_root/scripts/package-desktop-alpha.sh" "$out_dir" "alpha-verify"

desktop_bin="$out_dir/bin/app_desktop"
if [[ -f "$desktop_bin.exe" ]]; then
  desktop_bin="$desktop_bin.exe"
fi

test -x "$desktop_bin"
"$desktop_bin" --smoke-launch

for plugin in repo_manager status history branches tags compare diagnostics; do
  plugin_bin="$out_dir/plugins/$plugin"
  if [[ -f "$plugin_bin.exe" ]]; then
    plugin_bin="$plugin_bin.exe"
  fi
  test -x "$plugin_bin"
done

test -f "$out_dir/resources/icon.svg"
test -f "$out_dir/share/applications/branchforge.desktop"
test -f "$out_dir/macos/BranchForge.app/Contents/Info.plist"
test -x "$out_dir/macos/BranchForge.app/Contents/MacOS/BranchForge"
test -f "$out_dir/windows/README.txt"
test -d "$out_dir/config"
test -d "$out_dir/logs"
test -d "$out_dir/crashlogs"

echo "desktop alpha verification passed: $out_dir"
