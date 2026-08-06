#!/usr/bin/env bash
set -euo pipefail

REPO_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
TEMP_DIR="$(mktemp -d)"
trap 'rm -rf "$TEMP_DIR"' EXIT

export AWM_PROMOTE_LOG="$TEMP_DIR/gh.log"
export WORKMAN_PROMOTE_NPM_LOG="$TEMP_DIR/npm.log"
GH_BIN="$REPO_ROOT/scripts/tests/fixtures/gh-promote" \
NPM_BIN="$REPO_ROOT/scripts/tests/fixtures/npm-promote" \
  "$REPO_ROOT/scripts/promote.sh" v1.2.3 >/dev/null

EXPECTED="release edit v1.2.3 --prerelease=false --latest"
ACTUAL="$(<"$AWM_PROMOTE_LOG")"
if [[ "$ACTUAL" != "$EXPECTED" ]]; then
  echo "expected: $EXPECTED" >&2
  echo "actual:   $ACTUAL" >&2
  exit 1
fi

EXPECTED_NPM="--prefix $REPO_ROOT/infra/update-host ci --ignore-scripts
--prefix $REPO_ROOT/infra/update-host exec -- wrangler whoami
--prefix $REPO_ROOT/infra/update-host run promote -- --version 1.2.3"
ACTUAL_NPM="$(<"$WORKMAN_PROMOTE_NPM_LOG")"
if [[ "$ACTUAL_NPM" != "$EXPECTED_NPM" ]]; then
  echo "expected npm calls: $EXPECTED_NPM" >&2
  echo "actual npm calls:   $ACTUAL_NPM" >&2
  exit 1
fi

if GH_BIN="$REPO_ROOT/scripts/tests/fixtures/gh-promote" \
  NPM_BIN="$REPO_ROOT/scripts/tests/fixtures/npm-promote" \
  "$REPO_ROOT/scripts/promote.sh" not-a-tag >/dev/null 2>&1; then
  echo "invalid tag unexpectedly accepted" >&2
  exit 1
fi
