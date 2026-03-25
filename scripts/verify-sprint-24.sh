#!/usr/bin/env bash
set -euo pipefail

repo_root="$(cd "$(dirname "$0")/.." && pwd)"
cd "$repo_root"

./scripts/check-deps.sh
./scripts/verify-sprint-22.sh
./scripts/verify-sprint-23.sh

package_out="$repo_root/target/tmp/sprint24-package"
./scripts/package-local.sh "$package_out"

checksums="$package_out/sha256sums.txt"
(
  cd "$package_out"
  sha256sum bin/app_host plugins/* > "$checksums"
)

[[ -f "$package_out/manifest.txt" ]]
[[ -f "$checksums" ]]
[[ -f "$repo_root/docs/process/release_notes_v1.0.0.md" ]]
[[ -f "$repo_root/docs/process/changelog_v1.0.0.md" ]]
[[ -f "$repo_root/docs/process/known_issues_and_support_v1.0.0.md" ]]
[[ -f "$repo_root/docs/process/release_regression_matrix_sprint24.md" ]]
[[ -f "$repo_root/docs/process/rc_signoff_sprint24.md" ]]

echo "Sprint 24 local verification passed"

