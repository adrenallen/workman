#!/usr/bin/env bash
set -euo pipefail

REPO_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
TEMP_DIR="$(mktemp -d)"
trap 'rm -rf "$TEMP_DIR"' EXIT

export CARGO_TARGET_DIR="$TEMP_DIR/target"
mkdir -p "$CARGO_TARGET_DIR/aarch64-apple-darwin/dist"
printf '#!/bin/sh\n' > "$CARGO_TARGET_DIR/aarch64-apple-darwin/dist/awm-desktop"
chmod +x "$CARGO_TARGET_DIR/aarch64-apple-darwin/dist/awm-desktop"

"$REPO_ROOT/scripts/tauri-dist-runner.sh" \
  build --release --target aarch64-apple-darwin --features custom

test -x "$CARGO_TARGET_DIR/aarch64-apple-darwin/release/awm-desktop"
cmp \
  "$CARGO_TARGET_DIR/aarch64-apple-darwin/dist/awm-desktop" \
  "$CARGO_TARGET_DIR/aarch64-apple-darwin/release/awm-desktop"
