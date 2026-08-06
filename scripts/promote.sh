#!/usr/bin/env bash
set -euo pipefail

TAG="${1:-}"
if [[ ! "$TAG" =~ ^v[0-9]+\.[0-9]+\.[0-9]+$ ]]; then
  echo "usage: scripts/promote.sh vX.Y.Z" >&2
  exit 2
fi

REPO_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
UPDATE_HOST_DIR="$REPO_ROOT/infra/update-host"
GH_BIN="${GH_BIN:-gh}"
NPM_BIN="${NPM_BIN:-npm}"
if ! "$GH_BIN" release view "$TAG" >/dev/null 2>&1; then
  echo "release $TAG does not exist; publish the tag-only prerelease first" >&2
  exit 1
fi

"$NPM_BIN" --prefix "$UPDATE_HOST_DIR" ci --ignore-scripts
"$NPM_BIN" --prefix "$UPDATE_HOST_DIR" exec -- wrangler whoami >/dev/null
"$GH_BIN" release edit "$TAG" --prerelease=false --latest
"$NPM_BIN" --prefix "$UPDATE_HOST_DIR" run promote -- --version "${TAG#v}"
if ! "$NPM_BIN" --prefix "$UPDATE_HOST_DIR" run prune -- --yes; then
  echo "warning: R2 retention prune failed; promotion succeeded and will not be rolled back" >&2
fi
echo "Promoted Workman $TAG to the stable channel and marked it latest."
