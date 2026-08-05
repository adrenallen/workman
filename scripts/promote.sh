#!/usr/bin/env bash
set -euo pipefail

TAG="${1:-}"
if [[ ! "$TAG" =~ ^v[0-9]+\.[0-9]+\.[0-9]+$ ]]; then
  echo "usage: scripts/promote.sh vX.Y.Z" >&2
  exit 2
fi

GH_BIN="${GH_BIN:-gh}"
if ! "$GH_BIN" release view "$TAG" >/dev/null 2>&1; then
  echo "release $TAG does not exist; publish the tag-only prerelease first" >&2
  exit 1
fi

"$GH_BIN" release edit "$TAG" --prerelease=false --latest
echo "Promoted $TAG to the stable channel and marked it latest."
