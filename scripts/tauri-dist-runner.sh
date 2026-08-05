#!/usr/bin/env bash
set -euo pipefail

# The release workflow has already compiled every shipping Rust package together with
# `cargo build --locked --profile dist`. Tauri still invokes a runner before bundling; this
# adapter verifies and mirrors that exact desktop executable instead of compiling it again.
TARGET=""
while (($#)); do
  case "$1" in
    --target)
      TARGET="${2:?--target requires a value}"
      shift
      ;;
    --target=*)
      TARGET="${1#--target=}"
      ;;
  esac
  shift
done

: "${CARGO_TARGET_DIR:?CARGO_TARGET_DIR must point to the shared release target directory}"
PROFILE_ROOT="$CARGO_TARGET_DIR"
if [[ -n "$TARGET" ]]; then
  PROFILE_ROOT="$PROFILE_ROOT/$TARGET"
fi
BINARY="${AWM_TAURI_BINARY:-awm-desktop}"
test -x "$PROFILE_ROOT/dist/$BINARY"
mkdir -p "$PROFILE_ROOT/release"
cp "$PROFILE_ROOT/dist/$BINARY" "$PROFILE_ROOT/release/$BINARY"
