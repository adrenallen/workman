#!/usr/bin/env bash
set -euo pipefail

REPO_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
RELEASE_SCRIPT="$REPO_ROOT/scripts/release.sh"

HELP="$($RELEASE_SCRIPT --help)"
[[ "$HELP" == *"--dry-run"* ]]
[[ "$HELP" == *"does not create a tag"* ]]
grep -q 'cargo zigbuild --locked --profile dist' "$RELEASE_SCRIPT"
grep -q -- '--prerelease' "$RELEASE_SCRIPT"
grep -q -- '--latest=false' "$RELEASE_SCRIPT"
grep -q 'scripts/promote.sh' "$RELEASE_SCRIPT"
