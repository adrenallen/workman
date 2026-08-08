#!/usr/bin/env bash
set -euo pipefail

REPO_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
WORKFLOW="$REPO_ROOT/.github/workflows/release.yml"
CI_WORKFLOW="$REPO_ROOT/.github/workflows/ci.yml"

if grep -q 'cargo test' "$WORKFLOW"; then
  echo "release workflow must not run tests" >&2
  exit 1
fi
[[ "$(grep -c 'cargo build --locked --profile dist' "$WORKFLOW")" == 2 ]]
[[ "$(grep -c -- '-p workmand -p workman-cli' "$WORKFLOW")" == 2 ]]
[[ "$(grep -c 'scripts/tauri-dist-runner.sh' "$WORKFLOW")" == 2 ]]
[[ "$(grep -c 'run: npm run build' "$WORKFLOW")" == 2 ]]
[[ "$(grep -c 'beforeBuildCommand' "$WORKFLOW")" == 2 ]]
[[ "$(grep -c 'Swatinem/rust-cache@' "$WORKFLOW")" == 2 ]]
[[ "$(grep -c 'dtolnay/rust-toolchain@2eae45db285e407f22119950686d47e1101e071b' "$WORKFLOW")" == 2 ]]
grep -q "save-if:.*workflow_dispatch.*refs/heads/main" "$WORKFLOW"
grep -q 'workflow_dispatch:' "$WORKFLOW"
grep -q 'workflow_dispatch:' "$CI_WORKFLOW"
grep -q 'workman-macos-arm64.zip' "$WORKFLOW"
grep -q 'workman-linux-x86_64.tar.gz' "$WORKFLOW"
if grep -qi 'awm' "$WORKFLOW"; then
  echo "release workflow must not expose pre-Workman asset names" >&2
  exit 1
fi
if grep -Eq '(^|[[:space:]])(push|pull_request|tags):' "$WORKFLOW" "$CI_WORKFLOW"; then
  echo "repository workflows must be dispatch-only" >&2
  exit 1
fi
if grep -q 'gh release' "$WORKFLOW"; then
  echo "the emergency workflow must not publish releases" >&2
  exit 1
fi
