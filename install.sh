#!/usr/bin/env bash

set -euo pipefail

repo_root=$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd -P)
install_home=${HOME:?HOME must be set}
bin_dir=${WORKMAN_INSTALL_BIN_DIR:-"$install_home/.local/bin"}
daemon_was_running=false
awm_daemon_was_running=false
gbuild_daemon_was_running=false
install_alias=${WORKMAN_INSTALL_ALIAS:-1}

if [[ -n "${WORKMAN_DATA_DIR:-}" ]]; then
  daemon_data_dir=$WORKMAN_DATA_DIR
elif [[ $(uname -s) == Darwin ]]; then
  daemon_data_dir="$install_home/Library/Application Support/workman"
elif [[ -n "${XDG_DATA_HOME:-}" ]]; then
  daemon_data_dir="$XDG_DATA_HOME/workman"
else
  daemon_data_dir="$install_home/.local/share/workman"
fi

if [[ $(uname -s) == Darwin ]]; then
  awm_daemon_data_dir="$install_home/Library/Application Support/awm"
  gbuild_daemon_data_dir="$install_home/Library/Application Support/gbuild"
elif [[ -n "${XDG_DATA_HOME:-}" ]]; then
  awm_daemon_data_dir="$XDG_DATA_HOME/awm"
  gbuild_daemon_data_dir="$XDG_DATA_HOME/gbuild"
else
  awm_daemon_data_dir="$install_home/.local/share/awm"
  gbuild_daemon_data_dir="$install_home/.local/share/gbuild"
fi

discovery_file="$daemon_data_dir/daemon.json"
if [[ -r "$discovery_file" ]]; then
  daemon_pid=$(sed -nE 's/.*"pid"[[:space:]]*:[[:space:]]*([0-9]+).*/\1/p' "$discovery_file" | head -n 1)
  if [[ "$daemon_pid" =~ ^[0-9]+$ ]] && kill -0 "$daemon_pid" 2>/dev/null; then
    daemon_was_running=true
  fi
fi

for legacy_name in awm gbuild; do
  if [[ "$legacy_name" == awm ]]; then
    legacy_data_dir=$awm_daemon_data_dir
  else
    legacy_data_dir=$gbuild_daemon_data_dir
  fi
  legacy_discovery_file="$legacy_data_dir/daemon.json"
  if [[ -r "$legacy_discovery_file" ]]; then
    legacy_daemon_pid=$(sed -nE 's/.*"pid"[[:space:]]*:[[:space:]]*([0-9]+).*/\1/p' "$legacy_discovery_file" | head -n 1)
    if [[ "$legacy_daemon_pid" =~ ^[0-9]+$ ]] && kill -0 "$legacy_daemon_pid" 2>/dev/null; then
      if [[ "$legacy_name" == awm ]]; then
        awm_daemon_was_running=true
      else
        gbuild_daemon_was_running=true
      fi
    fi
  fi
done

if [[ "$bin_dir" != /* ]]; then
  printf 'workman: install directory must be absolute: %s\n' "$bin_dir" >&2
  exit 1
fi

for required in cargo npm; do
  if ! command -v "$required" >/dev/null 2>&1; then
    printf 'workman: %s is required to install workman\n' "$required" >&2
    exit 1
  fi
done

printf '\n  Workman · local installer\n\n'
printf '  ▸ Installing desktop dependencies\n'
(
  cd "$repo_root/apps/desktop"
  npm install --no-audit --no-fund
)

printf '\n  ▸ Building release binaries\n'
(
  cd "$repo_root"
  cargo build --release -p workman-cli -p workmand
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
for binary in wrk workmand workman-desktop; do
  target="$release_dir/$binary"
  link="$bin_dir/$binary"
  if [[ ! -x "$target" ]]; then
    printf 'workman: release build did not produce %s\n' "$target" >&2
    exit 1
  fi
  if [[ -e "$link" && ! -L "$link" ]]; then
    printf 'workman: refusing to replace non-symlink %s\n' "$link" >&2
    exit 1
  fi
  ln -sfn "$target" "$link"
  printf '  ✓ Linked %-14s %s\n' "$binary" "$link"
done

for stale_binary in awm awmd awm-desktop gbuild gbuildd gbuild-desktop; do
  stale_link="$bin_dir/$stale_binary"
  if [[ -L "$stale_link" ]]; then
    unlink "$stale_link"
    printf '  ✓ Removed stale %-8s %s\n' "$stale_binary" "$stale_link"
  fi
done

if [[ "$install_alias" != 0 && "$install_alias" != false && "$install_alias" != no ]]; then
  alias_link="$bin_dir/workman"
  if [[ -e "$alias_link" && ! -L "$alias_link" ]]; then
    printf 'workman: refusing to replace non-symlink %s\n' "$alias_link" >&2
    exit 1
  fi
  ln -sfn "$bin_dir/wrk" "$alias_link"
  printf '  ✓ Linked %-14s %s\n' "workman → wrk" "$alias_link"
fi

printf '\n  ✓ Workman is installed\n'
if [[ "$daemon_was_running" == true ]]; then
  printf '\n  ⚠ A Workman daemon was already running during this install.\n'
  printf '    Restart it from the app banner or Settings to apply the new daemon version.\n'
  printf '    Restarting stops currently running project processes.\n'
fi
if [[ "$awm_daemon_was_running" == true ]]; then
  printf '\n  ⚠ The legacy awm daemon is still running. It was not stopped.\n'
  printf '    Start Workman when ready; first boot copies awm state into the Workman data directory.\n'
elif [[ "$gbuild_daemon_was_running" == true ]]; then
  printf '\n  ⚠ The legacy gbuild daemon is still running. It was not stopped.\n'
  printf '    Start Workman when ready; first boot copies gbuild state into the Workman data directory.\n'
fi
case ":${PATH:-}:" in
  *":$bin_dir:"*) ;;
  *)
    printf '\n  Add Workman to PATH in your shell profile:\n'
    printf '    export PATH="%s:$PATH"\n' "$bin_dir"
    ;;
esac

printf '\n  Next:\n'
printf '    wrk              # run in a project directory\n'
printf '    wrk app          # open the desktop workspace\n'
printf '    wrk mcp-setup    # connect Claude Code\n\n'
