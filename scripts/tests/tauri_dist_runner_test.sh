#!/usr/bin/env bash
set -euo pipefail

REPO_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
TEMP_DIR="$(mktemp -d)"
trap 'rm -rf "$TEMP_DIR"' EXIT

export CARGO_BIN="$TEMP_DIR/fake-cargo"
cat > "$CARGO_BIN" <<'EOF'
#!/usr/bin/env bash
set -euo pipefail
printf '%s\n' "$@" > "$CARGO_TARGET_DIR/cargo-args"
mkdir -p "$CARGO_TARGET_DIR/aarch64-apple-darwin/dist"
printf '#!/bin/sh\n' > "$CARGO_TARGET_DIR/aarch64-apple-darwin/dist/workman-desktop"
chmod +x "$CARGO_TARGET_DIR/aarch64-apple-darwin/dist/workman-desktop"
EOF
chmod +x "$CARGO_BIN"

export CARGO_TARGET_DIR="$TEMP_DIR/production-target"
mkdir -p "$CARGO_TARGET_DIR"
"$REPO_ROOT/scripts/tauri-dist-runner.sh" \
  build --release --target aarch64-apple-darwin --features tauri/custom-protocol,custom

test -x "$CARGO_TARGET_DIR/aarch64-apple-darwin/release/workman-desktop"
grep -qx -- '--profile' "$CARGO_TARGET_DIR/cargo-args"
grep -qx -- 'dist' "$CARGO_TARGET_DIR/cargo-args"
grep -qx -- 'aarch64-apple-darwin' "$CARGO_TARGET_DIR/cargo-args"
grep -qx -- 'tauri/custom-protocol,custom' "$CARGO_TARGET_DIR/cargo-args"
! grep -qx -- '--release' "$CARGO_TARGET_DIR/cargo-args"
cmp \
  "$CARGO_TARGET_DIR/aarch64-apple-darwin/dist/workman-desktop" \
  "$CARGO_TARGET_DIR/aarch64-apple-darwin/release/workman-desktop"

export CARGO_TARGET_DIR="$TEMP_DIR/dev-url-target"
mkdir -p "$CARGO_TARGET_DIR"
if "$REPO_ROOT/scripts/tauri-dist-runner.sh" \
  build --release --target aarch64-apple-darwin \
  >"$TEMP_DIR/dev-url.stdout" 2>"$TEMP_DIR/dev-url.stderr"; then
  echo "runner accepted a release build without tauri/custom-protocol" >&2
  exit 1
fi

grep -Fq 'refusing to build a release desktop without `tauri/custom-protocol`' \
  "$TEMP_DIR/dev-url.stderr"
grep -Fq 'generate_context! selects build.devUrl' "$TEMP_DIR/dev-url.stderr"
test ! -e "$CARGO_TARGET_DIR/cargo-args"
test ! -e "$CARGO_TARGET_DIR/aarch64-apple-darwin/release/workman-desktop"

export CARGO_TARGET_DIR="$TEMP_DIR/equals-feature-target"
mkdir -p "$CARGO_TARGET_DIR"
"$REPO_ROOT/scripts/tauri-dist-runner.sh" \
  build --release --target=aarch64-apple-darwin --features=tauri/custom-protocol
test -x "$CARGO_TARGET_DIR/aarch64-apple-darwin/release/workman-desktop"
