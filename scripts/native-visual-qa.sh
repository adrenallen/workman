#!/usr/bin/env bash
set -euo pipefail

usage() {
  cat >&2 <<'EOF'
Usage: scripts/native-visual-qa.sh --todo NNN --source-app PATH [--daemon-bin PATH] [--prepare-only]

Copies an unsigned/ad-hoc Workman bundle into a fresh per-todo /tmp root, assigns the
com.workman.todoNNN identity, persists its isolated data/config environment in Info.plist,
registers that exact copy with LaunchServices, and launches it unless --prepare-only is set.
EOF
  exit 64
}

TODO_ID=""
SOURCE_APP=""
DAEMON_BIN=""
PREPARE_ONLY=0
while (($#)); do
  case "$1" in
    --todo) TODO_ID="${2:?--todo requires a value}"; shift ;;
    --source-app) SOURCE_APP="${2:?--source-app requires a value}"; shift ;;
    --daemon-bin) DAEMON_BIN="${2:?--daemon-bin requires a value}"; shift ;;
    --prepare-only) PREPARE_ONLY=1 ;;
    -h|--help) usage ;;
    *) printf 'native visual QA: unknown argument: %s\n' "$1" >&2; usage ;;
  esac
  shift
done

[[ "$TODO_ID" =~ ^[0-9]+$ ]] || { printf 'native visual QA: --todo must contain digits only\n' >&2; exit 64; }
[[ -n "$SOURCE_APP" ]] || usage
SOURCE_APP="$(cd "$(dirname "$SOURCE_APP")" && pwd)/$(basename "$SOURCE_APP")"
[[ -d "$SOURCE_APP/Contents/MacOS" && -f "$SOURCE_APP/Contents/Info.plist" ]] || {
  printf 'native visual QA: source is not a macOS app bundle: %s\n' "$SOURCE_APP" >&2
  exit 65
}
case "$SOURCE_APP" in
  /Applications/*|"${HOME}/Applications/"*)
    printf 'native visual QA: refusing an installed stable/dev bundle; build a disposable source app first: %s\n' "$SOURCE_APP" >&2
    exit 65
    ;;
esac
if [[ -n "$DAEMON_BIN" ]]; then
  DAEMON_BIN="$(cd "$(dirname "$DAEMON_BIN")" && pwd)/$(basename "$DAEMON_BIN")"
  [[ -x "$DAEMON_BIN" ]] || { printf 'native visual QA: daemon binary is not executable: %s\n' "$DAEMON_BIN" >&2; exit 65; }
fi

signature="$({ /usr/bin/codesign -dv --verbose=2 "$SOURCE_APP"; } 2>&1 || true)"
if [[ "$signature" == *"TeamIdentifier="* && "$signature" != *"TeamIdentifier=not set"* ]]; then
  printf 'native visual QA: refusing to rewrite a signed release bundle\n' >&2
  exit 65
fi

QA_ROOT="$(mktemp -d "/tmp/workman-todo${TODO_ID}-qa.XXXXXX")"
APP_NAME="Workman Todo ${TODO_ID}.app"
QA_APP="$QA_ROOT/$APP_NAME"
DATA_DIR="$QA_ROOT/data"
CONFIG="$QA_ROOT/config.yml"
OPEN_CAPTURE="$QA_ROOT/browser-open.log"
BUNDLE_ID="com.workman.todo${TODO_ID}"
cleanup_on_error() {
  status=$?
  if ((status != 0)); then
    rm -rf "$QA_ROOT"
  fi
  exit "$status"
}
trap cleanup_on_error EXIT

/usr/bin/ditto "$SOURCE_APP" "$QA_APP"
mkdir -p "$DATA_DIR"
umask 077
printf 'agent_tools: []\n' > "$CONFIG"
: > "$OPEN_CAPTURE"
chmod 600 "$OPEN_CAPTURE"

PLIST="$QA_APP/Contents/Info.plist"
/usr/libexec/PlistBuddy -c "Set :CFBundleIdentifier $BUNDLE_ID" "$PLIST"
/usr/libexec/PlistBuddy -c "Set :CFBundleName Workman Todo $TODO_ID" "$PLIST"
if ! /usr/libexec/PlistBuddy -c "Set :CFBundleDisplayName Workman Todo $TODO_ID" "$PLIST" 2>/dev/null; then
  /usr/libexec/PlistBuddy -c "Add :CFBundleDisplayName string Workman Todo $TODO_ID" "$PLIST"
fi
/usr/libexec/PlistBuddy -c "Delete :LSEnvironment" "$PLIST" >/dev/null 2>&1 || true
/usr/libexec/PlistBuddy -c "Add :LSEnvironment dict" "$PLIST"
/usr/libexec/PlistBuddy -c "Add :LSEnvironment:WORKMAN_DATA_DIR string $DATA_DIR" "$PLIST"
/usr/libexec/PlistBuddy -c "Add :LSEnvironment:WORKMAN_CONFIG string $CONFIG" "$PLIST"
/usr/libexec/PlistBuddy -c "Add :LSEnvironment:WORKMAN_REQUIRE_EXPLICIT_DAEMON string 1" "$PLIST"
/usr/libexec/PlistBuddy -c "Add :LSEnvironment:WORKMAN_BROWSER_OPEN_CAPTURE string $OPEN_CAPTURE" "$PLIST"
if [[ -n "$DAEMON_BIN" ]]; then
  /usr/libexec/PlistBuddy -c "Add :LSEnvironment:WORKMAN_DAEMON_BIN string $DAEMON_BIN" "$PLIST"
fi

actual_id="$(/usr/libexec/PlistBuddy -c 'Print :CFBundleIdentifier' "$PLIST")"
actual_data="$(/usr/libexec/PlistBuddy -c 'Print :LSEnvironment:WORKMAN_DATA_DIR' "$PLIST")"
actual_config="$(/usr/libexec/PlistBuddy -c 'Print :LSEnvironment:WORKMAN_CONFIG' "$PLIST")"
actual_guard="$(/usr/libexec/PlistBuddy -c 'Print :LSEnvironment:WORKMAN_REQUIRE_EXPLICIT_DAEMON' "$PLIST")"
actual_capture="$(/usr/libexec/PlistBuddy -c 'Print :LSEnvironment:WORKMAN_BROWSER_OPEN_CAPTURE' "$PLIST")"
[[ "$actual_id" == "$BUNDLE_ID" && "$actual_data" == "$DATA_DIR" && "$actual_config" == "$CONFIG" && "$actual_guard" == 1 && "$actual_capture" == "$OPEN_CAPTURE" ]] || {
  printf 'native visual QA: persisted isolation contract did not verify\n' >&2
  exit 70
}

LSREGISTER="/System/Library/Frameworks/CoreServices.framework/Frameworks/LaunchServices.framework/Support/lsregister"
printf 'QA_ROOT=%s\nQA_APP=%s\nBUNDLE_ID=%s\nWORKMAN_DATA_DIR=%s\nWORKMAN_CONFIG=%s\nWORKMAN_BROWSER_OPEN_CAPTURE=%s\n' \
  "$QA_ROOT" "$QA_APP" "$BUNDLE_ID" "$DATA_DIR" "$CONFIG" "$OPEN_CAPTURE"
if [[ -n "$DAEMON_BIN" ]]; then
  printf 'WORKMAN_DAEMON_BIN=%s\n' "$DAEMON_BIN"
fi

if ((PREPARE_ONLY == 0)); then
  "$LSREGISTER" -u "$QA_APP" >/dev/null 2>&1 || true
  "$LSREGISTER" -f "$QA_APP"
  /usr/bin/open -na "$QA_APP"
fi
trap - EXIT
