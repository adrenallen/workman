#!/usr/bin/env bash
set -euo pipefail

REPO_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
TEMP_DIR="$(mktemp -d)"
QA_ROOT=""
CUSTOM_ROOT=""
cleanup() {
  [[ -z "$QA_ROOT" ]] || rm -rf "$QA_ROOT"
  [[ -z "$CUSTOM_ROOT" ]] || rm -rf "$CUSTOM_ROOT"
  rm -rf "$TEMP_DIR"
}
trap cleanup EXIT

SOURCE_APP="$TEMP_DIR/Workman Test.app"
mkdir -p "$SOURCE_APP/Contents/MacOS"
printf '#!/bin/sh\nexit 0\n' > "$SOURCE_APP/Contents/MacOS/workman-desktop"
chmod +x "$SOURCE_APP/Contents/MacOS/workman-desktop"
cat > "$SOURCE_APP/Contents/Info.plist" <<'EOF'
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0"><dict>
  <key>CFBundleExecutable</key><string>workman-desktop</string>
  <key>CFBundleIdentifier</key><string>com.workman.desktop</string>
  <key>CFBundleName</key><string>Workman</string>
</dict></plist>
EOF

output="$($REPO_ROOT/scripts/native-visual-qa.sh --todo 307 --source-app "$SOURCE_APP" --prepare-only)"
QA_ROOT="$(printf '%s\n' "$output" | awk -F= '$1 == "QA_ROOT" { sub(/^QA_ROOT=/, ""); print; exit }')"
QA_APP="$(printf '%s\n' "$output" | awk -F= '$1 == "QA_APP" { sub(/^QA_APP=/, ""); print; exit }')"
DATA_DIR="$(printf '%s\n' "$output" | awk -F= '$1 == "WORKMAN_DATA_DIR" { sub(/^WORKMAN_DATA_DIR=/, ""); print; exit }')"
CONFIG="$(printf '%s\n' "$output" | awk -F= '$1 == "WORKMAN_CONFIG" { sub(/^WORKMAN_CONFIG=/, ""); print; exit }')"
OPEN_CAPTURE="$(printf '%s\n' "$output" | awk -F= '$1 == "WORKMAN_BROWSER_OPEN_CAPTURE" { sub(/^WORKMAN_BROWSER_OPEN_CAPTURE=/, ""); print; exit }')"

[[ "$QA_ROOT" == /tmp/workman-todo307-qa.* ]]
test -d "$QA_APP"
test -d "$DATA_DIR"
test -f "$CONFIG"
test -f "$OPEN_CAPTURE"
[[ "$(/usr/libexec/PlistBuddy -c 'Print :CFBundleIdentifier' "$QA_APP/Contents/Info.plist")" == com.workman.todo307 ]]
[[ "$(/usr/libexec/PlistBuddy -c 'Print :LSEnvironment:WORKMAN_DATA_DIR' "$QA_APP/Contents/Info.plist")" == "$DATA_DIR" ]]
[[ "$(/usr/libexec/PlistBuddy -c 'Print :LSEnvironment:WORKMAN_CONFIG' "$QA_APP/Contents/Info.plist")" == "$CONFIG" ]]
[[ "$(/usr/libexec/PlistBuddy -c 'Print :LSEnvironment:WORKMAN_REQUIRE_EXPLICIT_DAEMON' "$QA_APP/Contents/Info.plist")" == 1 ]]
[[ "$(/usr/libexec/PlistBuddy -c 'Print :LSEnvironment:WORKMAN_BROWSER_OPEN_CAPTURE' "$QA_APP/Contents/Info.plist")" == "$OPEN_CAPTURE" ]]
/usr/bin/codesign --verify --deep --strict "$QA_APP"
FAKE_PATH="$(/usr/libexec/PlistBuddy -c 'Print :LSEnvironment:PATH' "$QA_APP/Contents/Info.plist")"
[[ "$FAKE_PATH" == "$QA_ROOT/fake-bin:/usr/bin:/bin:/usr/sbin:/sbin" ]]
grep -qx 'agent_tools: \[\]' "$CONFIG"
test ! -s "$OPEN_CAPTURE"
WORKMAN_BROWSER_OPEN_CAPTURE="$OPEN_CAPTURE" "$QA_ROOT/fake-bin/open" 'https://example.test/harness'
grep -qx 'https://example.test/harness' "$OPEN_CAPTURE"
if WORKMAN_BROWSER_OPEN_CAPTURE="$OPEN_CAPTURE" "$QA_ROOT/fake-bin/open" 'file:///tmp/private' >/dev/null 2>&1; then
  printf 'expected fake open to reject non-http URL\n' >&2
  exit 1
fi
test -z "$(find "$DATA_DIR" -mindepth 1 -print -quit)"

CUSTOM_ROOT="/tmp/qa-native-script-$RANDOM-$$"
custom_output="$($REPO_ROOT/scripts/native-visual-qa.sh --todo 307 --identity fix28 --qa-root "$CUSTOM_ROOT" --source-app "$SOURCE_APP" --prepare-only)"
CUSTOM_APP="$(printf '%s\n' "$custom_output" | awk -F= '$1 == "QA_APP" { sub(/^QA_APP=/, ""); print; exit }')"
[[ "$(printf '%s\n' "$custom_output" | awk -F= '$1 == "BUNDLE_ID" { print $2; exit }')" == com.workman.fix28 ]]
[[ "$(printf '%s\n' "$custom_output" | awk -F= '$1 == "QA_ROOT" { sub(/^QA_ROOT=/, ""); print; exit }')" == "$CUSTOM_ROOT" ]]
[[ "$(/usr/libexec/PlistBuddy -c 'Print :CFBundleIdentifier' "$CUSTOM_APP/Contents/Info.plist")" == com.workman.fix28 ]]
/usr/bin/codesign --verify --deep --strict "$CUSTOM_APP"

if "$REPO_ROOT/scripts/native-visual-qa.sh" --todo unsafe --source-app "$SOURCE_APP" --prepare-only >/dev/null 2>&1; then
  printf 'expected non-numeric todo id to fail\n' >&2
  exit 1
fi
