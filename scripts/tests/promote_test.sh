#!/usr/bin/env bash
set -euo pipefail

REPO_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
TEMP_DIR="$(mktemp -d)"
trap 'rm -rf "$TEMP_DIR"' EXIT

export AWM_PROMOTE_LOG="$TEMP_DIR/gh.log"
GH_BIN="$REPO_ROOT/scripts/tests/fixtures/gh-promote" \
  "$REPO_ROOT/scripts/promote.sh" v1.2.3 >/dev/null

EXPECTED="release edit v1.2.3 --prerelease=false --latest"
ACTUAL="$(<"$AWM_PROMOTE_LOG")"
if [[ "$ACTUAL" != "$EXPECTED" ]]; then
  echo "expected: $EXPECTED" >&2
  echo "actual:   $ACTUAL" >&2
  exit 1
fi

if GH_BIN="$REPO_ROOT/scripts/tests/fixtures/gh-promote" \
  "$REPO_ROOT/scripts/promote.sh" not-a-tag >/dev/null 2>&1; then
  echo "invalid tag unexpectedly accepted" >&2
  exit 1
fi
