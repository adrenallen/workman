#!/usr/bin/env bash
set -euo pipefail

REPO_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
TEMP_DIR="$(mktemp -d)"
trap 'rm -rf "$TEMP_DIR"' EXIT

# shellcheck source=../release-public-repository.sh
source "$REPO_ROOT/scripts/release-public-repository.sh"
GH_FIXTURE="$REPO_ROOT/scripts/tests/fixtures/gh-release-repository"
export WORKMAN_TEST_GH_LOG="$TEMP_DIR/gh.log"

grep -Fq 'source "$REPO_ROOT/scripts/release-public-repository.sh"' \
  "$REPO_ROOT/scripts/release.sh"
preflight_body="$(sed -n '/^preflight() {$/,/^}$/p' "$REPO_ROOT/scripts/release.sh")"
[[ "$(grep -c '^  verify_public_release_repository adrenallen/workman$' \
  <<<"$preflight_body")" == 1 ]]
release_flow="$(sed -n '/^mkdir -p "$OUTPUT_DIR"/,$p' "$REPO_ROOT/scripts/release.sh")"
preflight_line="$(grep -n '^preflight$' <<<"$release_flow" | cut -d: -f1)"
first_build_line="$(grep -n '^clear_obsolete_artifacts$' \
  <<<"$release_flow" | cut -d: -f1)"
[[ -n "$preflight_line" ]]
[[ -n "$first_build_line" ]]
((preflight_line < first_build_line))

WORKMAN_TEST_REPOSITORY_VISIBILITY=public \
  verify_public_release_repository adrenallen/workman "$GH_FIXTURE"

if WORKMAN_TEST_REPOSITORY_VISIBILITY=private \
  verify_public_release_repository adrenallen/workman "$GH_FIXTURE" \
    2>"$TEMP_DIR/private.log"; then
  echo "private release repository unexpectedly passed preflight" >&2
  exit 1
fi
grep -Fq 'GitHub repository adrenallen/workman is private' "$TEMP_DIR/private.log"
grep -Fq 'update and download URLs are unauthenticated' "$TEMP_DIR/private.log"
grep -Fq 'require a public repository' "$TEMP_DIR/private.log"

if WORKMAN_TEST_REPOSITORY_VISIBILITY=internal \
  verify_public_release_repository adrenallen/workman "$GH_FIXTURE" \
    2>"$TEMP_DIR/internal.log"; then
  echo "internal release repository unexpectedly passed preflight" >&2
  exit 1
fi
grep -Fq 'GitHub repository adrenallen/workman is internal' "$TEMP_DIR/internal.log"

if WORKMAN_TEST_REPOSITORY_VISIBILITY=unavailable \
  verify_public_release_repository adrenallen/workman "$GH_FIXTURE" \
    2>"$TEMP_DIR/unavailable.log"; then
  echo "unverifiable release repository unexpectedly passed preflight" >&2
  exit 1
fi
grep -Fq 'could not verify GitHub repository visibility for adrenallen/workman' \
  "$TEMP_DIR/unavailable.log"
grep -Fq 'refusing to release' "$TEMP_DIR/unavailable.log"

[[ "$(wc -l < "$WORKMAN_TEST_GH_LOG" | tr -d ' ')" == 4 ]]
[[ "$(sort -u "$WORKMAN_TEST_GH_LOG")" == \
  'api repos/adrenallen/workman --jq .visibility' ]]
