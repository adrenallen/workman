#!/usr/bin/env bash

set -euo pipefail

repo_root=$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd -P)
install_home=${HOME:?HOME must be set}
bin_dir=${AWM_INSTALL_BIN_DIR:-"$install_home/.local/bin"}
daemon_was_running=false
legacy_daemon_was_running=false

if [[ -n "${AWM_DATA_DIR:-}" ]]; then
  daemon_data_dir=$AWM_DATA_DIR
elif [[ $(uname -s) == Darwin ]]; then
  daemon_data_dir="$install_home/Library/Application Support/awm"
elif [[ -n "${XDG_DATA_HOME:-}" ]]; then
  daemon_data_dir="$XDG_DATA_HOME/awm"
else
  daemon_data_dir="$install_home/.local/share/awm"
fi

if [[ $(uname -s) == Darwin ]]; then
  legacy_daemon_data_dir="$install_home/Library/Application Support/gbuild"
elif [[ -n "${XDG_DATA_HOME:-}" ]]; then
  legacy_daemon_data_dir="$XDG_DATA_HOME/gbuild"
else
  legacy_daemon_data_dir="$install_home/.local/share/gbuild"
fi

discovery_file="$daemon_data_dir/daemon.json"
if [[ -r "$discovery_file" ]]; then
  daemon_pid=$(sed -nE 's/.*"pid"[[:space:]]*:[[:space:]]*([0-9]+).*/\1/p' "$discovery_file" | head -n 1)
  if [[ "$daemon_pid" =~ ^[0-9]+$ ]] && kill -0 "$daemon_pid" 2>/dev/null; then
    daemon_was_running=true
  fi
fi

legacy_discovery_file="$legacy_daemon_data_dir/daemon.json"
if [[ -r "$legacy_discovery_file" ]]; then
  legacy_daemon_pid=$(sed -nE 's/.*"pid"[[:space:]]*:[[:space:]]*([0-9]+).*/\1/p' "$legacy_discovery_file" | head -n 1)
  if [[ "$legacy_daemon_pid" =~ ^[0-9]+$ ]] && kill -0 "$legacy_daemon_pid" 2>/dev/null; then
    legacy_daemon_was_running=true
  fi
fi

if [[ "$bin_dir" != /* ]]; then
  printf 'awm: install directory must be absolute: %s\n' "$bin_dir" >&2
  exit 1
fi

for required in cargo npm; do
  if ! command -v "$required" >/dev/null 2>&1; then
    printf 'awm: %s is required to install awm\n' "$required" >&2
    exit 1
  fi
done

printf '\n  awm · local installer\n\n'
printf '  ▸ Installing desktop dependencies\n'
(
  cd "$repo_root/apps/desktop"
  npm install --no-audit --no-fund
)

printf '\n  ▸ Building release binaries\n'
(
  cd "$repo_root"
  cargo build --release -p awm -p awmd
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
for binary in awm awmd awm-desktop; do
  target="$release_dir/$binary"
  link="$bin_dir/$binary"
  if [[ ! -x "$target" ]]; then
    printf 'awm: release build did not produce %s\n' "$target" >&2
    exit 1
  fi
  if [[ -e "$link" && ! -L "$link" ]]; then
    printf 'awm: refusing to replace non-symlink %s\n' "$link" >&2
    exit 1
  fi
  ln -sfn "$target" "$link"
  printf '  ✓ Linked %-14s %s\n' "$binary" "$link"
done

for stale_binary in gbuild gbuildd gbuild-desktop; do
  stale_link="$bin_dir/$stale_binary"
  if [[ -L "$stale_link" ]]; then
    unlink "$stale_link"
    printf '  ✓ Removed stale %-8s %s\n' "$stale_binary" "$stale_link"
  fi
done

printf '\n  ✓ awm is installed\n'
if [[ "$daemon_was_running" == true ]]; then
  printf '\n  ⚠ An awm daemon was already running during this install.\n'
  printf '    Restart it from the app banner or Settings to apply the new daemon version.\n'
  printf '    Restarting stops currently running project processes.\n'
fi
if [[ "$legacy_daemon_was_running" == true ]]; then
  printf '\n  ⚠ The legacy gbuild daemon is still running. It was not stopped.\n'
  printf '    Start awm when ready; first boot copies legacy state into the awm data directory.\n'
fi
case ":${PATH:-}:" in
  *":$bin_dir:"*) ;;
  *)
    printf '\n  Add awm to PATH in your shell profile:\n'
    printf '    export PATH="%s:$PATH"\n' "$bin_dir"
    ;;
esac

printf '\n  Next:\n'
printf '    awm              # run in a project directory\n'
printf '    awm app          # open the desktop workspace\n'
printf '    awm mcp-setup    # connect Claude Code\n\n'
