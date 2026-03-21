#!/usr/bin/env bash
set -euo pipefail

./scripts/check-deps.sh
cargo fmt --all --check
cargo clippy --workspace --all-targets --all-features -- -D warnings
cargo test --workspace


