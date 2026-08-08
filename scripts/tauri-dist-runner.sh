#!/usr/bin/env bash
set -euo pipefail

# Tauri invokes this runner with the production config/environment and a Cargo `--release`
# command. Compile the desktop in that environment while swapping only the profile to `dist`,
# then mirror it to the release path where Tauri's bundler expects to find it. A desktop binary
# compiled before Tauri starts retains the configured devUrl and produces a blank application.
PRODUCTION_FEATURE=false
EXPECT_FEATURE_LIST=false
for argument in "$@"; do
  if [[ "$EXPECT_FEATURE_LIST" == true ]]; then
    feature_list="${argument//,/ }"
    for feature in $feature_list; do
      [[ "$feature" == tauri/custom-protocol ]] && PRODUCTION_FEATURE=true
    done
    EXPECT_FEATURE_LIST=false
    continue
  fi

  case "$argument" in
    --features|-F) EXPECT_FEATURE_LIST=true ;;
    --features=*)
      feature_list="${argument#--features=}"
      feature_list="${feature_list//,/ }"
      for feature in $feature_list; do
        [[ "$feature" == tauri/custom-protocol ]] && PRODUCTION_FEATURE=true
      done
      ;;
    -F*)
      feature_list="${argument#-F}"
      feature_list="${feature_list//,/ }"
      for feature in $feature_list; do
        [[ "$feature" == tauri/custom-protocol ]] && PRODUCTION_FEATURE=true
      done
      ;;
  esac
done

if [[ "$PRODUCTION_FEATURE" != true ]]; then
  cat >&2 <<'EOF'
tauri dist runner: refusing to build a release desktop without `tauri/custom-protocol`.
Without Tauri's production protocol feature, generate_context! selects build.devUrl and the
packaged application opens a blank development WebView. Run this build through `tauri build`;
do not package a desktop binary compiled by a plain Cargo invocation.
EOF
  exit 78
fi

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
BINARY="${WORKMAN_TAURI_BINARY:-workman-desktop}"
test -x "$PROFILE_ROOT/dist/$BINARY"
mkdir -p "$PROFILE_ROOT/release"
cp "$PROFILE_ROOT/dist/$BINARY" "$PROFILE_ROOT/release/$BINARY"
