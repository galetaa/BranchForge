#!/usr/bin/env bash
set -euo pipefail

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
out_dir="${1:-$repo_root/target/tmp/desktop-alpha}"
channel="${2:-alpha}"

build_bins=(
  app_desktop
  repo_manager
  status
  history
  branches
  tags
  compare
  diagnostics
)

cargo_args=(build --release)
for bin in "${build_bins[@]}"; do
  cargo_args+=(-p "$bin")
done

cd "$repo_root"
cargo "${cargo_args[@]}"

rm -rf "$out_dir"
mkdir -p "$out_dir/bin" "$out_dir/plugins" "$out_dir/resources" "$out_dir/config" "$out_dir/logs" "$out_dir/crashlogs"

copy_binary() {
  local name="$1"
  local target="$2"
  local source="$repo_root/target/release/$name"
  if [[ -f "$source.exe" ]]; then
    source="$source.exe"
    target="$target.exe"
  fi
  cp "$source" "$target"
  chmod +x "$target"
}

copy_binary app_desktop "$out_dir/bin/app_desktop"
for plugin in repo_manager status history branches tags compare diagnostics; do
  copy_binary "$plugin" "$out_dir/plugins/$plugin"
done

cp "$repo_root/crates/app_desktop/assets/icon.svg" "$out_dir/resources/icon.svg"

cat > "$out_dir/README.txt" <<'EOF'
BranchForge Desktop alpha package

Run:
  ./bin/app_desktop

Smoke check:
  ./bin/app_desktop --smoke-launch

Bundled plugins are in ./plugins. Config, logs, and crashlogs directories are
created in this package for alpha verification; the app also uses platform
native config locations when run normally.
EOF

built_utc="$(date -u '+%Y-%m-%dT%H:%M:%SZ')"
cat > "$out_dir/manifest.json" <<EOF
{
  "product": "BranchForge Desktop",
  "version": "$(cargo metadata --no-deps --format-version 1 | sed -n 's/.*"version":"\([^"]*\)".*/\1/p' | head -1)",
  "channel": "$channel",
  "built_utc": "$built_utc",
  "layout": "desktop-alpha-v1",
  "binary": "bin/app_desktop",
  "plugins": ["repo_manager", "status", "history", "branches", "tags", "compare", "diagnostics"]
}
EOF

mkdir -p "$out_dir/share/applications"
cat > "$out_dir/share/applications/branchforge.desktop" <<EOF
[Desktop Entry]
Type=Application
Name=BranchForge
Comment=Native Git client
Exec=$out_dir/bin/app_desktop
Icon=$out_dir/resources/icon.svg
Categories=Development;RevisionControl;
Terminal=false
EOF

mkdir -p "$out_dir/macos/BranchForge.app/Contents/MacOS" "$out_dir/macos/BranchForge.app/Contents/Resources"
cp "$out_dir/bin/app_desktop" "$out_dir/macos/BranchForge.app/Contents/MacOS/BranchForge"
cp "$out_dir/resources/icon.svg" "$out_dir/macos/BranchForge.app/Contents/Resources/icon.svg"
cat > "$out_dir/macos/BranchForge.app/Contents/Info.plist" <<'EOF'
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
  <key>CFBundleExecutable</key>
  <string>BranchForge</string>
  <key>CFBundleIdentifier</key>
  <string>dev.branchforge.desktop</string>
  <key>CFBundleName</key>
  <string>BranchForge</string>
  <key>CFBundlePackageType</key>
  <string>APPL</string>
  <key>CFBundleShortVersionString</key>
  <string>1.0.1</string>
</dict>
</plist>
EOF

mkdir -p "$out_dir/windows"
cat > "$out_dir/windows/README.txt" <<'EOF'
Windows alpha layout:
  bin/app_desktop.exe
  plugins/*.exe

Zip the package root for manual Windows smoke testing.
EOF

echo "desktop alpha package created at $out_dir"
