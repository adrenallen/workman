#!/usr/bin/env bash
set -euo pipefail

# Tauri invokes this runner with the production config/environment and a Cargo `--release`
# command. Compile the desktop in that environment while swapping only the profile to `dist`,
# then mirror it to the release path where Tauri's bundler expects to find it. A desktop binary
# compiled before Tauri starts retains the configured devUrl and produces a blank application.
TARGET=""
CARGO_ARGS=()
while (($#)); do
  case "$1" in
    --release)
      ;;
    --target)
      TARGET="${2:?--target requires a value}"
      CARGO_ARGS+=("$1" "$TARGET")
      shift
      ;;
    --target=*)
      TARGET="${1#--target=}"
      CARGO_ARGS+=("$1")
      ;;
    *) CARGO_ARGS+=("$1") ;;
  esac
  shift
done

: "${CARGO_TARGET_DIR:?CARGO_TARGET_DIR must point to the shared release target directory}"
"${CARGO_BIN:-cargo}" "${CARGO_ARGS[@]}" --profile dist

PROFILE_ROOT="$CARGO_TARGET_DIR"
if [[ -n "$TARGET" ]]; then
  PROFILE_ROOT="$PROFILE_ROOT/$TARGET"
fi
BINARY="${AWM_TAURI_BINARY:-awm-desktop}"
test -x "$PROFILE_ROOT/dist/$BINARY"
mkdir -p "$PROFILE_ROOT/release"
cp "$PROFILE_ROOT/dist/$BINARY" "$PROFILE_ROOT/release/$BINARY"
