#!/usr/bin/env bash
set -euo pipefail

usage() {
  cat <<'EOF'
Usage: scripts/release.sh [--dry-run] <version>

Build every awm release artifact locally. --dry-run builds and verifies the complete artifact
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
OUTPUT_DIR="${AWM_RELEASE_OUTPUT_DIR:-$REPO_ROOT/release/$TAG}"
WORK_DIR="$OUTPUT_DIR/.work"
LOG_DIR="$OUTPUT_DIR/logs"
TIMINGS_FILE="$OUTPUT_DIR/build-timings.tsv"
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
  for tool in cargo rustup npm git gh jq tar ditto file shasum awk; do require "$tool"; done
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
  cargo build --locked --profile dist --target "$MACOS_TARGET" -p awmd -p awm
  CARGO_TARGET_DIR="$REPO_ROOT/target" npm --prefix apps/desktop run tauri -- build --ci --no-sign \
    --config '{"build":{"beforeBuildCommand":""}}' \
    --runner "$REPO_ROOT/scripts/tauri-dist-runner.sh" \
    --target "$MACOS_TARGET" --bundles app

  local target_dir="$REPO_ROOT/target/$MACOS_TARGET/dist"
  local app="$REPO_ROOT/target/$MACOS_TARGET/release/bundle/macos/awm.app"
  "$target_dir/awm" --version
  "$target_dir/awmd" --help >/dev/null
  test -d "$app"

  local package_dir="$WORK_DIR/macos-bin"
  rm -rf "$package_dir"
  mkdir -p "$package_dir"
  install -m 755 "$target_dir/awm" "$package_dir/awm"
  install -m 755 "$target_dir/awmd" "$package_dir/awmd"
  tar -C "$package_dir" -czf "$OUTPUT_DIR/awm-macos-arm64.tar.gz" awm awmd
  rm -f "$OUTPUT_DIR/awm-desktop-macos-arm64.zip"
  ditto -c -k --sequesterRsrc --keepParent "$app" "$OUTPUT_DIR/awm-desktop-macos-arm64.zip"
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
  local target label package_dir target_dir
  for target in x86_64-unknown-linux-musl aarch64-unknown-linux-musl; do
    case "$target" in
      x86_64-*) label=x86_64 ;;
      aarch64-*) label=arm64 ;;
    esac
    cargo zigbuild --locked --profile dist --target "$target" -p awmd -p awm
    target_dir="$REPO_ROOT/target/$target/dist"
    verify_static_linux_binary "$target_dir/awm" "$label"
    verify_static_linux_binary "$target_dir/awmd" "$label"
    package_dir="$WORK_DIR/linux-$label-bin"
    rm -rf "$package_dir"
    mkdir -p "$package_dir"
    install -m 755 "$target_dir/awm" "$package_dir/awm"
    install -m 755 "$target_dir/awmd" "$package_dir/awmd"
    tar -C "$package_dir" -czf "$OUTPUT_DIR/awm-linux-${label}.tar.gz" awm awmd
  done
  record_stage linux-static "$started"
}

build_linux_desktop() {
  local started=$SECONDS
  log "Experimental Linux desktop bundles (best effort)"
  # Optional desktop bundles must never survive from an older source revision
  # when Docker is unavailable (or a current build fails partway through).
  rm -f \
    "$OUTPUT_DIR/awm-desktop-linux-x86_64.AppImage" \
    "$OUTPUT_DIR/awm-desktop-linux-x86_64.deb" \
    "$OUTPUT_DIR/awm-desktop-linux-arm64.AppImage" \
    "$OUTPUT_DIR/awm-desktop-linux-arm64.deb"

  if ! command -v docker >/dev/null || ! docker info >/dev/null 2>&1; then
    warn "Docker/OrbStack is unavailable; skipping experimental Linux desktop bundles"
    printf 'linux-desktop\tskipped\n' >> "$TIMINGS_FILE"
    return 0
  fi

  local platform label destination log_file
  for platform in linux/arm64 linux/amd64; do
    case "$platform" in
      linux/arm64) label=arm64 ;;
      linux/amd64) label=x86_64 ;;
    esac
    destination="$WORK_DIR/docker-$label"
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
      install -m 755 "$destination/awm-desktop-linux-${label}.AppImage" "$OUTPUT_DIR/"
      install -m 644 "$destination/awm-desktop-linux-${label}.deb" "$OUTPUT_DIR/"
      printf '    built Linux desktop %s\n' "$label"
    else
      warn "Linux desktop $label build failed; see $log_file (continuing without it)"
    fi
  done
  record_stage linux-desktop "$started"
}

write_release_metadata() {
  local started=$SECONDS
  log "Checksums and release notes"
  local artifacts=(
    awm-macos-arm64.tar.gz
    awm-desktop-macos-arm64.zip
    awm-linux-x86_64.tar.gz
    awm-linux-arm64.tar.gz
  )
  local optional
  for optional in \
    awm-desktop-linux-x86_64.AppImage \
    awm-desktop-linux-x86_64.deb \
    awm-desktop-linux-arm64.AppImage \
    awm-desktop-linux-arm64.deb; do
    [[ -f "$OUTPUT_DIR/$optional" ]] && artifacts+=("$optional")
  done
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
    printf '# awm %s\n\n' "$TAG"
    printf '## Install\n\n```sh\n'
    printf 'git clone --branch "%s" --depth 1 https://github.com/adrenallen/awm.git && cd awm && ./install.sh\n' "$TAG"
    printf '```\n\n'
    printf '## macOS arm64\n\n'
    printf 'The desktop app is unsigned and not notarized. After unzipping, Control-click **awm.app** and choose **Open**. If macOS still quarantines it, run:\n\n'
    printf '```sh\nxattr -dr com.apple.quarantine /Applications/awm.app\n```\n\n'
    printf '## Linux — EXPERIMENTAL\n\n'
    printf 'Static CLI/daemon archives are provided for x86_64 and arm64. AppImage and Debian desktop bundles are best-effort local builds and may be absent when the container build is unavailable.\n\n'
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
      --title "awm $TAG" \
      --notes-file "$OUTPUT_DIR/release-notes.md" \
      --prerelease \
      --latest=false
  else
    gh release create "$TAG" "${assets[@]}" \
      --target "$(git rev-parse HEAD)" \
      --title "awm $TAG" \
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
ensure_linux_tools
build_macos
build_linux_binaries
build_linux_desktop
write_release_metadata
publish_release
record_stage total "$TOTAL_STARTED"

log "Release artifacts"
find "$OUTPUT_DIR" -maxdepth 1 -type f -print | sort
if [[ "$DRY_RUN" == false ]]; then
  printf '\nVerify the prerelease, then promote it with:\n  scripts/promote.sh %s\n' "$TAG"
fi
