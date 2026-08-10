#!/usr/bin/env bash
set -euo pipefail

REPO_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
TEST_ROOT="$(mktemp -d "${TMPDIR:-/tmp}/workman-installer-test.XXXXXX")"
TEST_HOME="$TEST_ROOT/home"
VERSION="9.8.7"
DURABLE_BIN="$TEST_HOME/.local/share/workman/dist/$VERSION/bin"

cleanup() {
  rm -rf "$TEST_ROOT"
}
trap cleanup EXIT

make_bundle() {
  local bundle_dir="$1" daemon_marker="$2"
  mkdir -p "$bundle_dir/bin"
  install -m 755 "$REPO_ROOT/scripts/release-assets/install.sh" "$bundle_dir/install.sh"
  cat >"$bundle_dir/bin/wrk" <<'EOF'
#!/bin/sh
printf 'workman 9.8.7\n'
EOF
  cat >"$bundle_dir/bin/workmand" <<EOF
#!/bin/sh
printf '%s\n' '$daemon_marker'
EOF
  chmod 755 "$bundle_dir/bin/wrk" "$bundle_dir/bin/workmand"
}

mkdir -p "$TEST_HOME/.local/bin"
ln -s "$TEST_ROOT/deleted-download/bin/wrk" "$TEST_HOME/.local/bin/wrk"
ln -s "$TEST_ROOT/deleted-download/bin/workmand" "$TEST_HOME/.local/bin/workmand"

FIRST_BUNDLE="$TEST_ROOT/extraction-one"
make_bundle "$FIRST_BUNDLE" first-install
HOME="$TEST_HOME" PATH="/usr/bin:/bin" "$FIRST_BUNDLE/install.sh" </dev/null
rm -rf "$FIRST_BUNDLE"

observed_version="$("$TEST_HOME/.local/bin/wrk" --version)"
[[ "$observed_version" == "workman $VERSION" ]]
[[ "$(readlink "$TEST_HOME/.local/bin/wrk")" == "$DURABLE_BIN/wrk" ]]
[[ "$(readlink "$TEST_HOME/.local/bin/workmand")" == "$DURABLE_BIN/workmand" ]]

SECOND_BUNDLE="$TEST_ROOT/extraction-two"
make_bundle "$SECOND_BUNDLE" second-install
HOME="$TEST_HOME" PATH="/usr/bin:/bin" "$SECOND_BUNDLE/install.sh" </dev/null
rm -rf "$SECOND_BUNDLE"

[[ "$("$TEST_HOME/.local/bin/wrk" --version)" == "workman $VERSION" ]]
[[ "$("$TEST_HOME/.local/bin/workmand")" == "second-install" ]]
[[ "$(readlink "$TEST_HOME/.local/bin/wrk")" == "$DURABLE_BIN/wrk" ]]
[[ "$(readlink "$TEST_HOME/.local/bin/workmand")" == "$DURABLE_BIN/workmand" ]]

printf 'installer durability regression: %s (extraction removed; rerun converged)\n' \
  "$observed_version"
