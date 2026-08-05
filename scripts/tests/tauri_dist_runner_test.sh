#!/usr/bin/env bash
set -euo pipefail

REPO_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
TEMP_DIR="$(mktemp -d)"
trap 'rm -rf "$TEMP_DIR"' EXIT

export CARGO_TARGET_DIR="$TEMP_DIR/target"
export CARGO_BIN="$TEMP_DIR/fake-cargo"
mkdir -p "$CARGO_TARGET_DIR"
cat > "$CARGO_BIN" <<'EOF'
#!/usr/bin/env bash
set -euo pipefail
printf '%s\n' "$@" > "$CARGO_TARGET_DIR/cargo-args"
mkdir -p "$CARGO_TARGET_DIR/aarch64-apple-darwin/dist"
printf '#!/bin/sh\n' > "$CARGO_TARGET_DIR/aarch64-apple-darwin/dist/awm-desktop"
chmod +x "$CARGO_TARGET_DIR/aarch64-apple-darwin/dist/awm-desktop"
EOF
chmod +x "$CARGO_BIN"

"$REPO_ROOT/scripts/tauri-dist-runner.sh" \
  build --release --target aarch64-apple-darwin --features custom

test -x "$CARGO_TARGET_DIR/aarch64-apple-darwin/release/awm-desktop"
grep -qx -- '--profile' "$CARGO_TARGET_DIR/cargo-args"
grep -qx -- 'dist' "$CARGO_TARGET_DIR/cargo-args"
grep -qx -- 'aarch64-apple-darwin' "$CARGO_TARGET_DIR/cargo-args"
grep -qx -- 'custom' "$CARGO_TARGET_DIR/cargo-args"
! grep -qx -- '--release' "$CARGO_TARGET_DIR/cargo-args"
cmp \
  "$CARGO_TARGET_DIR/aarch64-apple-darwin/dist/awm-desktop" \
  "$CARGO_TARGET_DIR/aarch64-apple-darwin/release/awm-desktop"
