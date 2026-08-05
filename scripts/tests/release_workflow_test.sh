#!/usr/bin/env bash
set -euo pipefail

REPO_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
WORKFLOW="$REPO_ROOT/.github/workflows/release.yml"

if grep -q 'cargo test' "$WORKFLOW"; then
  echo "release workflow must not run tests" >&2
  exit 1
fi
[[ "$(grep -c 'cargo build --locked --profile dist' "$WORKFLOW")" == 2 ]]
[[ "$(grep -c -- '-p awmd -p awm -p awm-desktop' "$WORKFLOW")" == 2 ]]
[[ "$(grep -c 'scripts/tauri-dist-runner.sh' "$WORKFLOW")" == 2 ]]
[[ "$(grep -c 'run: npm run build' "$WORKFLOW")" == 2 ]]
[[ "$(grep -c 'beforeBuildCommand' "$WORKFLOW")" == 2 ]]
[[ "$(grep -c 'Swatinem/rust-cache@' "$WORKFLOW")" == 2 ]]
[[ "$(grep -c 'dtolnay/rust-toolchain@2eae45db285e407f22119950686d47e1101e071b' "$WORKFLOW")" == 2 ]]
grep -q "save-if:.*workflow_dispatch.*refs/heads/main" "$WORKFLOW"
