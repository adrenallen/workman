#!/usr/bin/env bash

set -euo pipefail

repo_root=$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd -P)
install_home=${HOME:?HOME must be set}
bin_dir=${GBUILD_INSTALL_BIN_DIR:-"$install_home/.local/bin"}

if [[ "$bin_dir" != /* ]]; then
  printf 'gbuild: install directory must be absolute: %s\n' "$bin_dir" >&2
  exit 1
fi

for required in cargo npm; do
  if ! command -v "$required" >/dev/null 2>&1; then
    printf 'gbuild: %s is required to install gbuild\n' "$required" >&2
    exit 1
  fi
done

printf '\n  gbuild · local installer\n\n'
printf '  ▸ Installing desktop dependencies\n'
(
  cd "$repo_root/apps/desktop"
  npm install --no-audit --no-fund
)

printf '\n  ▸ Building release binaries\n'
(
  cd "$repo_root"
  cargo build --release -p gbuild-cli -p gbuildd
)

printf '\n  ▸ Building desktop application\n'
(
  cd "$repo_root/apps/desktop"
  # The Tauri CLI runs beforeBuildCommand and embeds frontendDist in the binary.
  # A separate Vite build followed by cargo build can reuse a stale Rust artifact.
  npm run tauri -- build --no-bundle
)

if [[ -n "${CARGO_TARGET_DIR:-}" ]]; then
  if [[ "$CARGO_TARGET_DIR" = /* ]]; then
    release_dir="$CARGO_TARGET_DIR/release"
  else
    release_dir="$repo_root/$CARGO_TARGET_DIR/release"
  fi
else
  release_dir="$repo_root/target/release"
fi
release_dir=$(cd "$release_dir" && pwd -P)

mkdir -p "$bin_dir"
for binary in gbuild gbuildd gbuild-desktop; do
  target="$release_dir/$binary"
  link="$bin_dir/$binary"
  if [[ ! -x "$target" ]]; then
    printf 'gbuild: release build did not produce %s\n' "$target" >&2
    exit 1
  fi
  if [[ -e "$link" && ! -L "$link" ]]; then
    printf 'gbuild: refusing to replace non-symlink %s\n' "$link" >&2
    exit 1
  fi
  ln -sfn "$target" "$link"
  printf '  ✓ Linked %-14s %s\n' "$binary" "$link"
done

printf '\n  ✓ gbuild is installed\n'
case ":${PATH:-}:" in
  *":$bin_dir:"*) ;;
  *)
    printf '\n  Add gbuild to PATH in your shell profile:\n'
    printf '    export PATH="%s:$PATH"\n' "$bin_dir"
    ;;
esac

printf '\n  Next:\n'
printf '    gbuild              # run in a project directory\n'
printf '    gbuild app          # open the desktop workspace\n'
printf '    gbuild mcp-setup    # connect Claude Code\n\n'
