#!/usr/bin/env bash
set -euo pipefail

usage() {
  cat <<'EOF'
Usage: scripts/release.sh [--dry-run] <version>

Build every Workman release artifact locally. --dry-run builds and verifies the complete artifact
set but does not create a tag, push, or publish a GitHub Release.
EOF
}

DRY_RUN=false
VERSION=""
while (($#)); do
  case "$1" in
    --dry-run) DRY_RUN=true ;;
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
TAG="v$VERSION"
OUTPUT_DIR="${WORKMAN_RELEASE_OUTPUT_DIR:-$REPO_ROOT/release/$TAG}"
WORK_DIR="$OUTPUT_DIR/.work"
LOG_DIR="$OUTPUT_DIR/logs"
TIMINGS_FILE="$OUTPUT_DIR/build-timings.tsv"
RELEASE_ASSETS_DIR="$REPO_ROOT/scripts/release-assets"
MACOS_TARGET=aarch64-apple-darwin
ZIGBUILD_VERSION=0.23.0

log() { printf '\n==> %s\n' "$*"; }
warn() { printf 'warning: %s\n' "$*" >&2; }
require() { command -v "$1" >/dev/null || { echo "required tool not found: $1" >&2; exit 1; }; }

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
  for tool in cargo rustup npm git gh jq tar ditto zip unzip file shasum awk; do require "$tool"; done
  [[ "$(uname -s)" == Darwin ]] || { echo "local releases must run on macOS" >&2; exit 1; }
  [[ "$(uname -m)" == arm64 ]] || { echo "local releases require Apple silicon" >&2; exit 1; }
  [[ "$(git branch --show-current)" == main ]] || { echo "release must run from main" >&2; exit 1; }

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
    git fetch --quiet origin main
    [[ "$(git rev-parse HEAD)" == "$(git rev-parse refs/remotes/origin/main)" ]] || {
      echo "main must be pushed and synchronized with origin/main before publishing" >&2
      exit 1
    }
  fi
}

clear_obsolete_artifacts() {
  # These split desktop assets were used before platform bundles became unified. Removing them
  # from a reused output directory prevents an old artifact from being uploaded accidentally.
  rm -f \
    "$OUTPUT_DIR/awm-desktop-macos-arm64.zip" \
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
  CARGO_TARGET_DIR="$REPO_ROOT/target" npm --prefix apps/desktop run tauri -- build --ci --no-sign \
    --config '{"build":{"beforeBuildCommand":""}}' \
    --runner "$REPO_ROOT/scripts/tauri-dist-runner.sh" \
    --target "$MACOS_TARGET" --bundles app

  local target_dir="$REPO_ROOT/target/$MACOS_TARGET/dist"
  local app="$REPO_ROOT/target/$MACOS_TARGET/release/bundle/macos/Workman.app"
  "$target_dir/wrk" --version
  "$target_dir/workmand" --help >/dev/null
  test -d "$app"

  local package_dir="$WORK_DIR/macos-bundle"
  rm -rf "$package_dir"
  mkdir -p "$package_dir/bin"
  install -m 755 "$target_dir/wrk" "$package_dir/bin/wrk"
  install -m 755 "$target_dir/workmand" "$package_dir/bin/workmand"
  ditto "$app" "$package_dir/Workman.app"
  add_bundle_guides "$package_dir" macos

  rm -f "$OUTPUT_DIR/workman-macos-arm64.zip"
  (
    cd "$package_dir"
    COPYFILE_DISABLE=1 zip -qry --symlinks "$OUTPUT_DIR/workman-macos-arm64.zip" .
  )
  cp "$OUTPUT_DIR/workman-macos-arm64.zip" "$OUTPUT_DIR/awm-desktop-macos-arm64.zip"

  # One-release bridge: the updater shipped in v0.1.0 requests this old asset name and root
  # layout. New updater builds consume bin/ from the unified ZIP above.
  local legacy_dir="$WORK_DIR/macos-legacy-v0.1.0"
  rm -rf "$legacy_dir"
  mkdir -p "$legacy_dir"
  install -m 755 "$target_dir/wrk" "$legacy_dir/awm"
  install -m 755 "$target_dir/workmand" "$legacy_dir/awmd"
  tar -C "$legacy_dir" -czf "$OUTPUT_DIR/awm-macos-arm64.tar.gz" awm awmd
  record_stage macos "$started"
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
      install -m 755 \
        "$destination/workman-desktop-linux-${label}.AppImage" \
        "$OUTPUT_DIR/awm-desktop-linux-${label}.AppImage"
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
    entries=(GETTING-STARTED.md install.sh bin Workman.AppImage)
    tar -C "$package_dir" -czf "$OUTPUT_DIR/workman-linux-${label}.tar.gz" "${entries[@]}"

    # One-release bridge for v0.1.0 clients. The old updater requires root-level awm/awmd
    # entries and requests awm-linux-<arch>.tar.gz from the redirected repository API.
    local legacy_dir="$WORK_DIR/linux-$label-legacy-v0.1.0"
    rm -rf "$legacy_dir"
    mkdir -p "$legacy_dir"
    install -m 755 "$package_dir/bin/wrk" "$legacy_dir/awm"
    install -m 755 "$package_dir/bin/workmand" "$legacy_dir/awmd"
    tar -C "$legacy_dir" -czf "$OUTPUT_DIR/awm-linux-${label}.tar.gz" awm awmd
  done
  record_stage packaging "$started"
}

verify_bundle_layouts() {
  local started=$SECONDS
  log "Platform bundle layouts"
  local roots expected label entries mac_entries

  mac_entries="$(unzip -Z1 "$OUTPUT_DIR/workman-macos-arm64.zip")"
  roots="$(printf '%s\n' "$mac_entries" | awk -F/ 'NF { print $1 }' | sort -u)"
  expected="$(printf '%s\n' GETTING-STARTED.md Workman.app bin install.sh | sort)"
  [[ "$roots" == "$expected" ]] || {
    echo "macOS bundle has unexpected top-level entries:" >&2
    printf '%s\n' "$roots" >&2
    exit 1
  }
  for entry in GETTING-STARTED.md install.sh bin/wrk bin/workmand; do
    grep -qx "$entry" <<<"$mac_entries"
  done
  grep -q '^Workman\.app/' <<<"$mac_entries"

  for label in x86_64 arm64; do
    entries="$(tar -tzf "$OUTPUT_DIR/workman-linux-${label}.tar.gz")"
    roots="$(printf '%s\n' "$entries" | awk -F/ 'NF { print $1 }' | sort -u)"
    expected="$(printf '%s\n' GETTING-STARTED.md Workman.AppImage bin install.sh | sort)"
    [[ "$roots" == "$expected" ]] || {
      echo "Linux $label bundle has unexpected top-level entries:" >&2
      printf '%s\n' "$roots" >&2
      exit 1
    }
    for entry in GETTING-STARTED.md install.sh Workman.AppImage bin/wrk bin/workmand; do
      grep -qx "$entry" <<<"$entries"
    done
  done

  for legacy_archive in \
    awm-macos-arm64.tar.gz \
    awm-linux-x86_64.tar.gz \
    awm-linux-arm64.tar.gz; do
    entries="$(tar -tzf "$OUTPUT_DIR/$legacy_archive" | sort)"
    expected="$(printf '%s\n' awm awmd | sort)"
    [[ "$entries" == "$expected" ]] || {
      echo "$legacy_archive does not match the v0.1.0 root-level binary contract" >&2
      printf '%s\n' "$entries" >&2
      exit 1
    }
  done
  record_stage layouts "$started"
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
    awm-macos-arm64.tar.gz
    awm-desktop-macos-arm64.zip
    awm-linux-x86_64.tar.gz
    awm-linux-arm64.tar.gz
    awm-desktop-linux-x86_64.AppImage
    awm-desktop-linux-arm64.AppImage
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
    printf 'Each platform archive contains `GETTING-STARTED.md`; read it first. The macOS app is unsigned, so its guide includes the Control-click and `xattr` first-run steps.\n\n'
    printf '> The `awm-*.tar.gz` and `awm-desktop-*` files are one-release compatibility aliases solely so published v0.1.0 clients can make the hop and find the replacement desktop bundle. New installs must use the `workman-*` assets.\n\n'
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
}

mkdir -p "$OUTPUT_DIR" "$WORK_DIR" "$LOG_DIR"
: > "$TIMINGS_FILE"
TOTAL_STARTED=$SECONDS
preflight
clear_obsolete_artifacts
ensure_linux_tools
build_macos
build_linux_binaries
build_linux_desktop
package_linux_bundles
verify_bundle_layouts
write_release_metadata
publish_release
record_stage total "$TOTAL_STARTED"

log "Release artifacts"
find "$OUTPUT_DIR" -maxdepth 1 -type f -print | sort
if [[ "$DRY_RUN" == false ]]; then
  printf '\nVerify the prerelease, then promote it with:\n  scripts/promote.sh %s\n' "$TAG"
fi
