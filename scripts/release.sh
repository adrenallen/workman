#!/usr/bin/env bash
set -euo pipefail

usage() {
  cat <<'EOF'
Usage: scripts/release.sh [--dry-run | --signing-test] <version>

Build every Workman release artifact locally. --dry-run builds and verifies the complete artifact
set but does not create a tag, push, or publish a GitHub Release or R2 update.

--signing-test builds, signs, notarizes, staples, and verifies only the macOS bundle. It never
creates a tag or publishes an artifact.
EOF
}

DRY_RUN=false
SIGNING_TEST=false
VERSION=""
while (($#)); do
  case "$1" in
    --dry-run) DRY_RUN=true ;;
    --signing-test) DRY_RUN=true; SIGNING_TEST=true ;;
    --help|-h) usage; exit 0 ;;
    -*) echo "unknown option: $1" >&2; usage >&2; exit 2 ;;
    *)
      if [[ -n "$VERSION" ]]; then
        echo "only one version may be provided" >&2
        exit 2
      fi
      VERSION="${1#v}"
      ;;
  esac
  shift
done

if [[ ! "$VERSION" =~ ^[0-9]+\.[0-9]+\.[0-9]+$ ]]; then
  echo "version must be X.Y.Z" >&2
  usage >&2
  exit 2
fi

REPO_ROOT="$(git rev-parse --show-toplevel)"
cd "$REPO_ROOT"
# shellcheck source=release-public-repository.sh
source "$REPO_ROOT/scripts/release-public-repository.sh"
TAG="v$VERSION"
OUTPUT_DIR="${WORKMAN_RELEASE_OUTPUT_DIR:-$REPO_ROOT/release/$TAG}"
WORK_DIR="$OUTPUT_DIR/.work"
LOG_DIR="$OUTPUT_DIR/logs"
TIMINGS_FILE="$OUTPUT_DIR/build-timings.tsv"
RELEASE_ASSETS_DIR="$REPO_ROOT/scripts/release-assets"
UPDATE_HOST_DIR="$REPO_ROOT/infra/update-host"
MACOS_TARGET=aarch64-apple-darwin
ZIGBUILD_VERSION=0.23.0
RELEASE_ENV_FILE="${WORKMAN_RELEASE_ENV_FILE:-$HOME/.workman-release.env}"
NOTARY_TIMEOUT="${WORKMAN_NOTARY_TIMEOUT:-2h}"

if [[ -f "$RELEASE_ENV_FILE" ]]; then
  # This is a trusted, local-only shell file so release credentials never enter git.
  set -a
  # shellcheck disable=SC1090
  source "$RELEASE_ENV_FILE"
  set +a
fi

log() { printf '\n==> %s\n' "$*"; }
warn() { printf 'warning: %s\n' "$*" >&2; }
require() { command -v "$1" >/dev/null || { echo "required tool not found: $1" >&2; exit 1; }; }

require_signing_credentials() {
  local missing=() variable
  for variable in \
    APPLE_SIGNING_IDENTITY \
    APPLE_API_KEY_PATH \
    APPLE_API_KEY \
    APPLE_API_ISSUER \
    APPLE_TEAM_ID; do
    [[ -n "${!variable:-}" ]] || missing+=("$variable")
  done
  if ((${#missing[@]})); then
    echo "macOS signing configuration is incomplete." >&2
    printf 'Missing %s\n' "${missing[@]}" >&2
    echo "Set the variables in $RELEASE_ENV_FILE or the release environment." >&2
    exit 1
  fi
  [[ -r "$APPLE_API_KEY_PATH" ]] || {
    echo "App Store Connect key is missing or unreadable: $APPLE_API_KEY_PATH" >&2
    exit 1
  }

  local identities
  identities="$(security find-identity -v -p codesigning)"
  grep -Fq "\"$APPLE_SIGNING_IDENTITY\"" <<<"$identities" || {
    echo "Developer ID signing identity is not available: $APPLE_SIGNING_IDENTITY" >&2
    printf '%s\n' "$identities" >&2
    exit 1
  }

  xcrun notarytool history \
    --key "$APPLE_API_KEY_PATH" \
    --key-id "$APPLE_API_KEY" \
    --issuer "$APPLE_API_ISSUER" \
    --output-format json >/dev/null || {
      echo "App Store Connect notarization credentials were rejected" >&2
      exit 1
    }
}

record_stage() {
  local name="$1" start="$2"
  local elapsed=$((SECONDS - start))
  printf '%s\t%s\n' "$name" "$elapsed" >> "$TIMINGS_FILE"
  printf '    %s completed in %ss\n' "$name" "$elapsed"
}

version_stamp() {
  case "$1" in
    workspace) sed -n 's/^version = "\([^"]*\)"/\1/p' Cargo.toml | head -1 ;;
    desktop) jq -r .version apps/desktop/package.json ;;
    tauri) jq -r .version apps/desktop/src-tauri/tauri.conf.json ;;
  esac
}

preflight() {
  log "Preflight"
  for tool in cargo rustup npm git gh jq tar ditto zip unzip file shasum awk security codesign xcrun spctl plutil cmake; do require "$tool"; done
  [[ "$(uname -s)" == Darwin ]] || { echo "local releases must run on macOS" >&2; exit 1; }
  [[ "$(uname -m)" == arm64 ]] || { echo "local releases require Apple silicon" >&2; exit 1; }
  [[ "$(git branch --show-current)" == main ]] || { echo "release must run from main" >&2; exit 1; }
  verify_public_release_repository adrenallen/workman
  require_signing_credentials

  local dirty
  dirty="$(git status --porcelain)"
  if [[ -n "$dirty" ]]; then
    if [[ "$DRY_RUN" == true ]]; then
      warn "dry-run is using an uncommitted working tree"
    else
      echo "release requires a clean working tree" >&2
      exit 1
    fi
  fi

  for stamp in workspace desktop tauri; do
    local actual
    actual="$(version_stamp "$stamp")"
    [[ "$actual" == "$VERSION" ]] || {
      echo "$stamp version is $actual, expected $VERSION" >&2
      exit 1
    }
  done
  grep -qE "^## ${VERSION} - [0-9]{4}-[0-9]{2}-[0-9]{2}$" CHANGELOG.md || {
    echo "CHANGELOG.md has no dated section for $VERSION" >&2
    exit 1
  }
  gh auth status >/dev/null

  if [[ "$DRY_RUN" == false ]]; then
    npm --prefix "$UPDATE_HOST_DIR" ci --ignore-scripts
    npm --prefix "$UPDATE_HOST_DIR" exec -- wrangler whoami >/dev/null
    git fetch --quiet origin main
    [[ "$(git rev-parse HEAD)" == "$(git rev-parse refs/remotes/origin/main)" ]] || {
      echo "main must be pushed and synchronized with origin/main before publishing" >&2
      exit 1
    }
  fi
}

sign_macos_binary() {
  local binary="$1"
  codesign --force --timestamp --options runtime --sign "$APPLE_SIGNING_IDENTITY" "$binary"
  codesign --verify --strict --verbose=2 "$binary"
}

verify_recording_macos_metadata() {
  local app="$1" info="$1/Contents/Info.plist" entitlements
  [[ -n "$(plutil -extract NSMicrophoneUsageDescription raw -o - "$info")" ]] || {
    echo "macOS bundle is missing NSMicrophoneUsageDescription" >&2
    exit 1
  }
  [[ -n "$(plutil -extract NSScreenCaptureUsageDescription raw -o - "$info")" ]] || {
    echo "macOS bundle is missing NSScreenCaptureUsageDescription" >&2
    exit 1
  }
  entitlements="$(codesign -d --entitlements :- "$app" 2>/dev/null)"
  plutil -convert json -o - - <<<"$entitlements" \
    | jq -e '."com.apple.security.device.audio-input" == true' >/dev/null || {
    echo "macOS bundle is missing the audio-input entitlement" >&2
    exit 1
  }
}

create_macos_zip() {
  local package_dir="$1" destination="$2"
  rm -f "$destination"
  (
    cd "$package_dir"
    COPYFILE_DISABLE=1 zip -qry --symlinks "$destination" .
  )
}

notarize_macos_package() {
  local package_dir="$1"
  local app="$package_dir/Workman.app"
  local submission="$WORK_DIR/workman-macos-arm64-notary.zip"
  local result="$LOG_DIR/macos-notarization.json"
  create_macos_zip "$package_dir" "$submission"

  log "Submit macOS bundle for notarization"
  xcrun notarytool submit "$submission" \
    --key "$APPLE_API_KEY_PATH" \
    --key-id "$APPLE_API_KEY" \
    --issuer "$APPLE_API_ISSUER" \
    --wait \
    --timeout "$NOTARY_TIMEOUT" \
    --no-progress \
    --output-format json >"$result"

  local status submission_id
  status="$(jq -r '.status // empty' "$result")"
  submission_id="$(jq -r '.id // empty' "$result")"
  if [[ "$status" != Accepted ]]; then
    if [[ -n "$submission_id" ]]; then
      xcrun notarytool log "$submission_id" \
        --key "$APPLE_API_KEY_PATH" \
        --key-id "$APPLE_API_KEY" \
        --issuer "$APPLE_API_ISSUER" \
        "$LOG_DIR/macos-notarization-log.json" || true
    fi
    cat "$result" >&2
    echo "Apple notarization failed with status: ${status:-missing}" >&2
    exit 1
  fi
  printf '    notarization accepted: %s\n' "$submission_id"

  xcrun stapler staple -v "$app"
  xcrun stapler validate -v "$app"
}

verify_signed_macos_package() {
  local archive="$1"
  local verify_dir="$WORK_DIR/macos-signature-verification"
  rm -rf "$verify_dir"
  mkdir -p "$verify_dir"
  unzip -q "$archive" -d "$verify_dir"

  codesign --verify --deep --strict --verbose=2 "$verify_dir/Workman.app"
  verify_recording_macos_metadata "$verify_dir/Workman.app"
  codesign --verify --strict --verbose=2 "$verify_dir/bin/wrk"
  codesign --verify --strict --verbose=2 "$verify_dir/bin/workmand"
  xcrun stapler validate -v "$verify_dir/Workman.app"
  spctl -a -vv --type execute "$verify_dir/Workman.app"

  local team
  for executable in \
    "$verify_dir/Workman.app/Contents/MacOS/workman-desktop" \
    "$verify_dir/bin/wrk" \
    "$verify_dir/bin/workmand"; do
    team="$(codesign -d --verbose=4 "$executable" 2>&1 | sed -n 's/^TeamIdentifier=//p')"
    [[ "$team" == "$APPLE_TEAM_ID" ]] || {
      echo "unexpected signing team for $executable: ${team:-missing}" >&2
      exit 1
    }
  done
}

clear_obsolete_artifacts() {
  # These split desktop assets were used before platform bundles became unified. Removing them
  # from a reused output directory prevents an old artifact from being uploaded accidentally.
  rm -f \
    "$OUTPUT_DIR/awm-macos-arm64.tar.gz" \
    "$OUTPUT_DIR/awm-desktop-macos-arm64.zip" \
    "$OUTPUT_DIR/awm-linux-x86_64.tar.gz" \
    "$OUTPUT_DIR/awm-linux-arm64.tar.gz" \
    "$OUTPUT_DIR/awm-desktop-linux-x86_64.AppImage" \
    "$OUTPUT_DIR/awm-desktop-linux-x86_64.deb" \
    "$OUTPUT_DIR/awm-desktop-linux-arm64.AppImage" \
    "$OUTPUT_DIR/awm-desktop-linux-arm64.deb" \
    "$OUTPUT_DIR/awm-linux-x86_64.deb" \
    "$OUTPUT_DIR/awm-linux-arm64.deb" \
    "$OUTPUT_DIR/workman-desktop-macos-arm64.zip" \
    "$OUTPUT_DIR/workman-desktop-linux-x86_64.AppImage" \
    "$OUTPUT_DIR/workman-desktop-linux-x86_64.deb" \
    "$OUTPUT_DIR/workman-desktop-linux-arm64.AppImage" \
    "$OUTPUT_DIR/workman-desktop-linux-arm64.deb"
}

add_bundle_guides() {
  local package_dir="$1" platform="$2"
  install -m 755 "$RELEASE_ASSETS_DIR/install.sh" "$package_dir/install.sh"
  install -m 644 \
    "$RELEASE_ASSETS_DIR/GETTING-STARTED-${platform}.md" \
    "$package_dir/GETTING-STARTED.md"
  node "$REPO_ROOT/scripts/generate-third-party-notices.mjs" \
    "$package_dir/THIRD_PARTY_NOTICES.md"
}

ensure_linux_tools() {
  log "Linux cross-build tools"
  if ! command -v zig >/dev/null; then
    require brew
    brew install zig
  fi
  if ! command -v cargo-zigbuild >/dev/null; then
    cargo install cargo-zigbuild --version "$ZIGBUILD_VERSION" --locked
  fi
  for target in x86_64-unknown-linux-musl aarch64-unknown-linux-musl; do
    if ! rustup target list --installed | grep -qx "$target"; then
      rustup target add "$target"
    fi
  done
  zig version
  cargo zigbuild --help >/dev/null
  printf 'cargo-zigbuild %s\n' "$ZIGBUILD_VERSION"
}

build_macos() {
  local started=$SECONDS
  log "macOS arm64 binaries and desktop"
  npm --prefix apps/desktop ci
  npm --prefix apps/desktop run build
  cargo build --locked --profile dist --target "$MACOS_TARGET" -p workmand -p workman-cli
  local tauri_signing_config
  if [[ "$SIGNING_TEST" == true ]]; then
    tauri_signing_config="$(jq -cn \
      --arg identity "$APPLE_SIGNING_IDENTITY" \
      --arg identifier "${WORKMAN_SIGNING_TEST_BUNDLE_ID:-com.workman.todo417}" \
      '{"identifier":$identifier,"build":{"beforeBuildCommand":""},"bundle":{"macOS":{"signingIdentity":$identity,"hardenedRuntime":true}}}')"
  else
    tauri_signing_config="$(jq -cn --arg identity "$APPLE_SIGNING_IDENTITY" \
      '{"build":{"beforeBuildCommand":""},"bundle":{"macOS":{"signingIdentity":$identity,"hardenedRuntime":true}}}')"
  fi
  env -u APPLE_API_KEY -u APPLE_API_ISSUER -u APPLE_API_KEY_PATH \
    CARGO_TARGET_DIR="$REPO_ROOT/target" npm --prefix apps/desktop run tauri -- build --ci \
    --config "$tauri_signing_config" \
    --runner "$REPO_ROOT/scripts/tauri-dist-runner.sh" \
    --target "$MACOS_TARGET" --bundles app

  local target_dir="$REPO_ROOT/target/$MACOS_TARGET/dist"
  local app="$REPO_ROOT/target/$MACOS_TARGET/release/bundle/macos/Workman.app"
  "$target_dir/wrk" --version
  "$target_dir/workmand" --help >/dev/null
  test -d "$app"
  sign_macos_binary "$target_dir/wrk"
  sign_macos_binary "$target_dir/workmand"
  codesign --verify --deep --strict --verbose=2 "$app"
  verify_recording_macos_metadata "$app"

  local package_dir="$WORK_DIR/macos-bundle"
  rm -rf "$package_dir"
  mkdir -p "$package_dir/bin"
  install -m 755 "$target_dir/wrk" "$package_dir/bin/wrk"
  install -m 755 "$target_dir/workmand" "$package_dir/bin/workmand"
  ditto "$app" "$package_dir/Workman.app"
  add_bundle_guides "$package_dir" macos

  notarize_macos_package "$package_dir"
  create_macos_zip "$package_dir" "$OUTPUT_DIR/workman-macos-arm64.zip"
  verify_signed_macos_package "$OUTPUT_DIR/workman-macos-arm64.zip"
  record_stage macos "$started"
}

verify_macos_bundle_layout() {
  local roots expected mac_entries
  mac_entries="$(unzip -Z1 "$OUTPUT_DIR/workman-macos-arm64.zip")"
  roots="$(printf '%s\n' "$mac_entries" | awk -F/ 'NF { print $1 }' | sort -u)"
  expected="$(printf '%s\n' GETTING-STARTED.md THIRD_PARTY_NOTICES.md Workman.app bin install.sh | sort)"
  [[ "$roots" == "$expected" ]] || {
    echo "macOS bundle has unexpected top-level entries:" >&2
    printf '%s\n' "$roots" >&2
    exit 1
  }
  for entry in GETTING-STARTED.md THIRD_PARTY_NOTICES.md install.sh bin/wrk bin/workmand; do
    grep -qx "$entry" <<<"$mac_entries"
  done
  grep -q '^Workman\.app/' <<<"$mac_entries"
}

verify_static_linux_binary() {
  local binary="$1" expected_arch="$2"
  local description
  description="$(file -b "$binary")"
  printf '    %s: %s\n' "$(basename "$binary")" "$description"
  case "$expected_arch" in
    x86_64) [[ "$description" == *"x86-64"* ]] ;;
    arm64) [[ "$description" == *"ARM aarch64"* ]] ;;
  esac
  [[ "$description" == *"statically linked"* || "$description" == *"static-pie linked"* ]]
}

build_linux_binaries() {
  local started=$SECONDS
  log "Static Linux CLI and daemon"
  local target label target_dir
  for target in x86_64-unknown-linux-musl aarch64-unknown-linux-musl; do
    case "$target" in
      x86_64-*) label=x86_64 ;;
      aarch64-*) label=arm64 ;;
    esac
    cargo zigbuild --locked --profile dist --target "$target" -p workmand -p workman-cli
    target_dir="$REPO_ROOT/target/$target/dist"
    verify_static_linux_binary "$target_dir/wrk" "$label"
    verify_static_linux_binary "$target_dir/workmand" "$label"
  done
  record_stage linux-static "$started"
}

build_linux_desktop() {
  local started=$SECONDS
  log "Experimental Linux desktop bundles"
  rm -rf "$WORK_DIR/linux-x86_64-desktop" "$WORK_DIR/linux-arm64-desktop"
  require docker
  docker info >/dev/null 2>&1 || {
    echo "Docker/OrbStack is required for the complete Linux release set" >&2
    exit 1
  }

  local platform label destination log_file
  for platform in linux/arm64 linux/amd64; do
    case "$platform" in
      linux/arm64) label=arm64 ;;
      linux/amd64) label=x86_64 ;;
    esac
    destination="$WORK_DIR/linux-$label-desktop"
    log_file="$LOG_DIR/linux-desktop-$label.log"
    rm -rf "$destination"
    mkdir -p "$destination"
    if docker build \
      --platform "$platform" \
      --file scripts/release-linux-desktop.Dockerfile \
      --target artifacts \
      --output "type=local,dest=$destination" \
      --progress plain \
      . >"$log_file" 2>&1; then
      install -m 755 \
        "$destination/workman-desktop-linux-${label}.AppImage" \
        "$destination/Workman.AppImage"
      install -m 755 \
        "$destination/workman-desktop-linux-${label}.AppImage" \
        "$OUTPUT_DIR/workman-linux-${label}.AppImage"
      install -m 644 \
        "$destination/workman-desktop-linux-${label}.deb" \
        "$OUTPUT_DIR/workman-linux-${label}.deb"
      printf '    built Linux desktop %s\n' "$label"
    else
      echo "Linux desktop $label build failed; see $log_file" >&2
      exit 1
    fi
  done
  record_stage linux-desktop "$started"
}

package_linux_bundles() {
  local started=$SECONDS
  log "Unified Linux platform bundles"
  local label target package_dir desktop_dir
  local entries=()
  for label in x86_64 arm64; do
    case "$label" in
      x86_64) target=x86_64-unknown-linux-musl ;;
      arm64) target=aarch64-unknown-linux-musl ;;
    esac
    package_dir="$WORK_DIR/linux-$label-bundle"
    desktop_dir="$WORK_DIR/linux-$label-desktop"
    rm -rf "$package_dir"
    mkdir -p "$package_dir/bin"
    install -m 755 "$REPO_ROOT/target/$target/dist/wrk" "$package_dir/bin/wrk"
    install -m 755 "$REPO_ROOT/target/$target/dist/workmand" "$package_dir/bin/workmand"
    add_bundle_guides "$package_dir" linux
    install -m 755 "$desktop_dir/Workman.AppImage" "$package_dir/Workman.AppImage"
    entries=(GETTING-STARTED.md THIRD_PARTY_NOTICES.md install.sh bin Workman.AppImage)
    tar -C "$package_dir" -czf "$OUTPUT_DIR/workman-linux-${label}.tar.gz" "${entries[@]}"
  done
  record_stage packaging "$started"
}

verify_bundle_layouts() {
  local started=$SECONDS
  log "Platform bundle layouts"
  local roots expected label entries

  verify_macos_bundle_layout

  for label in x86_64 arm64; do
    entries="$(tar -tzf "$OUTPUT_DIR/workman-linux-${label}.tar.gz")"
    roots="$(printf '%s\n' "$entries" | awk -F/ 'NF { print $1 }' | sort -u)"
    expected="$(printf '%s\n' GETTING-STARTED.md THIRD_PARTY_NOTICES.md Workman.AppImage bin install.sh | sort)"
    [[ "$roots" == "$expected" ]] || {
      echo "Linux $label bundle has unexpected top-level entries:" >&2
      printf '%s\n' "$roots" >&2
      exit 1
    }
    for entry in GETTING-STARTED.md THIRD_PARTY_NOTICES.md install.sh Workman.AppImage bin/wrk bin/workmand; do
      grep -qx "$entry" <<<"$entries"
    done
  done

  record_stage layouts "$started"
}

verify_app_surface_update_hop() {
  local started=$SECONDS
  log "Dock-launched app update hop"
  "$REPO_ROOT/scripts/verify-app-surface-update.sh"
  record_stage app-surface-update "$started"
}

write_release_metadata() {
  local started=$SECONDS
  log "Checksums and release notes"
  local artifacts=(
    workman-macos-arm64.zip
    workman-linux-x86_64.tar.gz
    workman-linux-arm64.tar.gz
    workman-linux-x86_64.AppImage
    workman-linux-arm64.AppImage
    workman-linux-x86_64.deb
    workman-linux-arm64.deb
  )
  local artifact
  for artifact in "${artifacts[@]}"; do test -f "$OUTPUT_DIR/$artifact"; done

  (
    cd "$OUTPUT_DIR"
    shasum -a 256 "${artifacts[@]}" > SHA256SUMS
    shasum -a 256 -c SHA256SUMS
  )

  awk -v version="$VERSION" '
    $0 ~ "^## " version " - " { found = 1; next }
    found && /^## / { exit }
    found { print }
    END { if (!found) exit 2 }
  ' CHANGELOG.md > "$WORK_DIR/changelog-section.md"

  {
    printf '# Workman %s\n\n' "$TAG"
    printf '## Pick one download\n\n'
    printf -- '- **macOS Apple silicon:** `workman-macos-arm64.zip` — app, CLI, daemon, installer, and getting-started guide.\n'
    printf -- '- **Linux x86_64 (portable, experimental):** `workman-linux-x86_64.tar.gz` — AppImage, static CLI/daemon, installer, and guide.\n'
    printf -- '- **Linux arm64 (portable, experimental):** `workman-linux-arm64.tar.gz` — AppImage, static CLI/daemon, installer, and guide.\n'
    printf -- '- **Linux Debian package (experimental):** choose the matching standalone `.deb` instead of the portable archive.\n\n'
    printf 'Each platform archive contains `GETTING-STARTED.md`; read it first. After extracting, install the CLI and daemon with `./install.sh`.\n\n'
    printf '> **macOS trust:** Workman.app, `wrk`, and `workmand` are Developer ID signed and notarized. Browser-downloaded builds should open normally. Versions 0.1.4 and earlier were unsigned and may still require the legacy Gatekeeper workaround in their bundled guide.\n\n'
    printf '## Changes\n\n'
    cat "$WORK_DIR/changelog-section.md"
  } > "$OUTPUT_DIR/release-notes.md"
  record_stage metadata "$started"
}

publish_release() {
  if [[ "$DRY_RUN" == true ]]; then
    log "Dry run complete — tag and GitHub publication skipped"
    return 0
  fi

  log "Tag and publish prerelease"
  [[ -z "$(git status --porcelain)" ]] || { echo "tree changed during release build" >&2; exit 1; }
  if git rev-parse -q --verify "refs/tags/$TAG" >/dev/null; then
    [[ "$(git rev-list -n 1 "$TAG")" == "$(git rev-parse HEAD)" ]] || {
      echo "local tag $TAG points to a different commit" >&2
      exit 1
    }
  else
    git tag "$TAG"
  fi
  git push origin "refs/tags/$TAG"

  local assets=()
  local path
  while IFS= read -r path; do assets+=("$path"); done < <(
    find "$OUTPUT_DIR" -maxdepth 1 -type f \
      \( -name '*.tar.gz' -o -name '*.zip' -o -name '*.AppImage' -o -name '*.deb' -o -name SHA256SUMS \) \
      -print | sort
  )
  if gh release view "$TAG" >/dev/null 2>&1; then
    gh release upload "$TAG" "${assets[@]}" --clobber
    gh release edit "$TAG" \
      --title "Workman $TAG" \
      --notes-file "$OUTPUT_DIR/release-notes.md" \
      --prerelease \
      --latest=false
  else
    gh release create "$TAG" "${assets[@]}" \
      --target "$(git rev-parse HEAD)" \
      --title "Workman $TAG" \
      --notes-file "$OUTPUT_DIR/release-notes.md" \
      --prerelease \
      --latest=false \
      --verify-tag
  fi

  local published_at
  published_at="$(gh release view "$TAG" --json publishedAt --jq .publishedAt)"
  npm --prefix "$UPDATE_HOST_DIR" run publish -- release \
    --version "$VERSION" \
    --artifacts-dir "$OUTPUT_DIR" \
    --published-at "$published_at" \
    --notes-url "https://github.com/adrenallen/workman/releases/tag/$TAG" \
    --installer "$UPDATE_HOST_DIR/install.sh"
}

prune_r2_releases() {
  [[ "$DRY_RUN" == false ]] || return 0
  log "R2 retention"
  if ! npm --prefix "$UPDATE_HOST_DIR" run prune -- --yes; then
    warn "R2 retention prune failed; release publication succeeded and will not be rolled back"
  fi
}

mkdir -p "$OUTPUT_DIR" "$WORK_DIR" "$LOG_DIR"
: > "$TIMINGS_FILE"
TOTAL_STARTED=$SECONDS
preflight
clear_obsolete_artifacts
if [[ "$SIGNING_TEST" == false ]]; then
  ensure_linux_tools
fi
build_macos
if [[ "$SIGNING_TEST" == true ]]; then
  verify_macos_bundle_layout
  record_stage total "$TOTAL_STARTED"
  log "Signed macOS test complete — publication skipped"
  find "$OUTPUT_DIR" -maxdepth 1 -type f -print | sort
  exit 0
fi
build_linux_binaries
build_linux_desktop
package_linux_bundles
verify_bundle_layouts
verify_app_surface_update_hop
write_release_metadata
publish_release
prune_r2_releases
record_stage total "$TOTAL_STARTED"

log "Release artifacts"
find "$OUTPUT_DIR" -maxdepth 1 -type f -print | sort
if [[ "$DRY_RUN" == false ]]; then
  printf '\nVerify the prerelease, then promote it with:\n  scripts/promote.sh %s\n' "$TAG"
fi
