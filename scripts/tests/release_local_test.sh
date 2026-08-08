#!/usr/bin/env bash
set -euo pipefail

REPO_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
RELEASE_SCRIPT="$REPO_ROOT/scripts/release.sh"

HELP="$($RELEASE_SCRIPT --help)"
[[ "$HELP" == *"--dry-run"* ]]
[[ "$HELP" == *"--signing-test"* ]]
[[ "$HELP" == *"does not create a tag"* ]]
grep -q 'APPLE_SIGNING_IDENTITY' "$RELEASE_SCRIPT"
grep -q 'APPLE_API_KEY_PATH' "$RELEASE_SCRIPT"
grep -q 'notarytool submit' "$RELEASE_SCRIPT"
grep -q 'stapler staple' "$RELEASE_SCRIPT"
grep -q 'codesign --verify --deep --strict' "$RELEASE_SCRIPT"
grep -q 'spctl -a -vv' "$RELEASE_SCRIPT"
grep -q 'cargo zigbuild --locked --profile dist' "$RELEASE_SCRIPT"
grep -q 'workman-macos-arm64.zip' "$RELEASE_SCRIPT"
grep -q 'workman-linux-x86_64.tar.gz' "$RELEASE_SCRIPT"
grep -q 'workman-linux-arm64.AppImage' "$RELEASE_SCRIPT"
grep -q 'workman-linux-arm64.deb' "$RELEASE_SCRIPT"
grep -q 'bin/wrk' "$RELEASE_SCRIPT"
grep -q 'bin/workmand' "$RELEASE_SCRIPT"
grep -q 'Workman.app' "$RELEASE_SCRIPT"
grep -q 'package_linux_bundles' "$RELEASE_SCRIPT"
grep -q 'verify_bundle_layouts' "$RELEASE_SCRIPT"
grep -q -- '--prerelease' "$RELEASE_SCRIPT"
grep -q -- '--latest=false' "$RELEASE_SCRIPT"
grep -q 'scripts/promote.sh' "$RELEASE_SCRIPT"
grep -q 'infra/update-host' "$RELEASE_SCRIPT"
grep -q 'wrangler whoami' "$RELEASE_SCRIPT"
grep -q 'run publish -- release' "$RELEASE_SCRIPT"
grep -q 'run prune -- --yes' "$RELEASE_SCRIPT"
grep -q 'R2 retention prune failed; release publication succeeded' "$RELEASE_SCRIPT"
if awk '
  /^clear_obsolete_artifacts\(\) \{$/ { cleanup = 1; next }
  cleanup && /^}$/ { cleanup = 0; next }
  !cleanup { print }
' "$RELEASE_SCRIPT" | grep -qi 'awm'; then
  echo "release surfaces must not expose pre-Workman asset names" >&2
  exit 1
fi

test -x "$REPO_ROOT/scripts/release-assets/install.sh"
grep -q 'never run `workmand` by hand' \
  "$REPO_ROOT/scripts/release-assets/GETTING-STARTED-macos.md"
grep -Fq 'Developer ID signed and notarized' \
  "$REPO_ROOT/scripts/release-assets/GETTING-STARTED-macos.md"
grep -Fq 'Releases 0.1.4 and earlier were unsigned' \
  "$REPO_ROOT/scripts/release-assets/GETTING-STARTED-macos.md"
grep -q 'never run `workmand` by hand' \
  "$REPO_ROOT/scripts/release-assets/GETTING-STARTED-linux.md"
