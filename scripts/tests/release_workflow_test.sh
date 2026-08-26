#!/usr/bin/env bash
set -euo pipefail

REPO_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
WORKFLOW="$REPO_ROOT/.github/workflows/release.yml"
CI_WORKFLOW="$REPO_ROOT/.github/workflows/ci.yml"
SECURITY_WORKFLOW="$REPO_ROOT/.github/workflows/security.yml"

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
[[ "$(grep -c 'dtolnay/rust-toolchain@06e5a564a0556e338780f5aecf2e7dcc9b267f07' "$WORKFLOW")" == 2 ]]
grep -q "save-if:.*workflow_dispatch.*refs/heads/main" "$WORKFLOW"
grep -q 'workflow_dispatch:' "$WORKFLOW"
grep -q 'workflow_dispatch:' "$CI_WORKFLOW"
grep -q 'pull_request:' "$CI_WORKFLOW"
grep -q 'branches:' "$CI_WORKFLOW"
grep -q 'schedule:' "$SECURITY_WORKFLOW"
grep -q 'workflow_dispatch:' "$SECURITY_WORKFLOW"
if grep -Eq '(^|[[:space:]])(push|pull_request):' "$SECURITY_WORKFLOW"; then
  echo "scheduled security workflow must not duplicate CI checks" >&2
  exit 1
fi
grep -q 'name: Secret scan' "$CI_WORKFLOW"
grep -q 'name: Scheduled secret scan' "$SECURITY_WORKFLOW"
grep -q 'workman-macos-arm64.zip' "$WORKFLOW"
grep -q 'workman-linux-x86_64.tar.gz' "$WORKFLOW"
[[ "$(grep -c 'generate-third-party-notices.mjs' "$WORKFLOW")" == 2 ]]
grep -q 'THIRD_PARTY_NOTICES.md' "$WORKFLOW"
if grep -qi 'awm' "$WORKFLOW"; then
  echo "release workflow must not expose pre-Workman asset names" >&2
  exit 1
fi
if grep -Eq '(^|[[:space:]])(push|pull_request|tags):' "$WORKFLOW"; then
  echo "release workflow must remain dispatch-only" >&2
  exit 1
fi
if grep -RE 'uses: [^@[:space:]]+@v[0-9]' "$REPO_ROOT/.github/workflows"; then
  echo "GitHub Actions must be pinned to immutable commit SHAs" >&2
  exit 1
fi
if grep -q 'gh release' "$WORKFLOW"; then
  echo "the emergency workflow must not publish releases" >&2
  exit 1
fi
