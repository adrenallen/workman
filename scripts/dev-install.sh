#!/usr/bin/env bash

set -euo pipefail

usage() {
  cat <<'EOF'
Build and install the current Workman working tree as an isolated development identity.

Usage: scripts/dev-install.sh [--reset-permissions] [--no-relaunch]

Installs:
  ~/.local/bin/wrk-dev
  ~/.local/bin/workmand-dev
  ~/Applications/Workman Dev.app  (bundle id com.workman.dev)

Environment overrides:
  WORKMAN_DEV_BIN_DIR       Launcher directory (default: ~/.local/bin)
  WORKMAN_DEV_INSTALL_DIR   Private binary directory (default: ~/.local/share/workman-dev)
  WORKMAN_DEV_APP_PATH      App destination (default: ~/Applications/Workman Dev.app)
  WORKMAN_DEV_BUILD_DIR     Cargo/Tauri target directory (default: target)
  WORKMAN_DEV_RELAUNCH      Relaunch after install: 1 or 0 (default: 1)
  WORKMAN_DEV_RESET_PERMISSIONS
                            macOS capture permission handling: auto, 1, or 0
                            (default: auto; reset when the signing identity changes)
  WORKMAN_DEV_SIGNING_IDENTITY
                            macOS signing identity (default: first Apple Development
                            or Developer ID Application identity; use - for ad hoc)

Options:
  --reset-permissions       Reset Screen Recording and Microphone access even when
                            the installed app has the same signing identity
  --no-relaunch             Install without reopening Workman Dev
EOF
}

reset_permissions=${WORKMAN_DEV_RESET_PERMISSIONS:-auto}
relaunch=${WORKMAN_DEV_RELAUNCH:-1}
while (( $# > 0 )); do
  case "$1" in
    --help|-h) usage; exit 0 ;;
    --reset-permissions) reset_permissions=1 ;;
    --no-relaunch) relaunch=0 ;;
    *) printf 'workman dev: unknown option: %s\n' "$1" >&2; usage >&2; exit 2 ;;
  esac
  shift
done
case "$reset_permissions" in
  auto|0|1) ;;
  *) printf 'workman dev: WORKMAN_DEV_RESET_PERMISSIONS must be auto, 1, or 0\n' >&2; exit 2 ;;
esac
case "$relaunch" in
  0|1) ;;
  *) printf 'workman dev: WORKMAN_DEV_RELAUNCH must be 1 or 0\n' >&2; exit 2 ;;
esac

repo_root=$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")/.." && pwd -P)
install_home=${HOME:?HOME must be set}
bin_dir=${WORKMAN_DEV_BIN_DIR:-"$install_home/.local/bin"}
install_dir=${WORKMAN_DEV_INSTALL_DIR:-"$install_home/.local/share/workman-dev"}
app_path=${WORKMAN_DEV_APP_PATH:-"$install_home/Applications/Workman Dev.app"}
build_dir=${WORKMAN_DEV_BUILD_DIR:-"$repo_root/target"}
data_dir="$install_home/Library/Application Support/workman-dev"
bundle_id=com.workman.dev
lsregister=/System/Library/Frameworks/CoreServices.framework/Frameworks/LaunchServices.framework/Support/lsregister

for path in "$bin_dir" "$install_dir" "$app_path" "$build_dir"; do
  if [[ "$path" != /* ]]; then
    printf 'workman dev: install and build paths must be absolute: %s\n' "$path" >&2
    exit 1
  fi
done
if [[ $(uname -s) != Darwin ]]; then
  printf 'workman dev: Workman Dev.app currently requires macOS\n' >&2
  exit 1
fi
for required in cargo npm ditto codesign security /usr/bin/tccutil /usr/libexec/PlistBuddy "$lsregister"; do
  if [[ "$required" == /* ]]; then
    [[ -x "$required" ]] || { printf 'workman dev: required tool not found: %s\n' "$required" >&2; exit 1; }
  elif ! command -v "$required" >/dev/null 2>&1; then
    printf 'workman dev: required tool not found: %s\n' "$required" >&2
    exit 1
  fi
done

app_executable_name=workman-desktop
if [[ -f "$app_path/Contents/Info.plist" ]]; then
  app_executable_name=$(/usr/libexec/PlistBuddy -c 'Print :CFBundleExecutable' \
    "$app_path/Contents/Info.plist" 2>/dev/null || printf 'workman-desktop')
fi
app_executable="$app_path/Contents/MacOS/$app_executable_name"
discovery_file="$data_dir/daemon.json"

process_is_alive() {
  /bin/kill -0 "$1" 2>/dev/null
}

process_command() {
  /bin/ps -p "$1" -o command= 2>/dev/null \
    | /usr/bin/sed -e 's/^[[:space:]]*//'
}

process_is_descendant_of() {
  local ancestor=$1 current=$2 parent
  while (( current > 1 )); do
    [[ "$current" == "$ancestor" ]] && return 0
    parent=$(/bin/ps -p "$current" -o ppid= 2>/dev/null \
      | /usr/bin/tr -d '[:space:]') || return 1
    [[ "$parent" =~ ^[0-9]+$ ]] || return 1
    [[ "$parent" == "$current" ]] && return 1
    current=$parent
  done
  return 1
}

discovered_daemon_pid() {
  [[ -f "$discovery_file" ]] || return 0
  /usr/bin/sed -nE 's/.*"pid"[[:space:]]*:[[:space:]]*([0-9]+).*/\1/p' \
    "$discovery_file" 2>/dev/null | /usr/bin/sed -n '1p' || true
}

ensure_external_installer_shell() {
  local daemon_pid
  daemon_pid=$(discovered_daemon_pid)
  if [[ "$daemon_pid" =~ ^[0-9]+$ ]] \
    && process_is_alive "$daemon_pid" \
    && process_is_descendant_of "$daemon_pid" "$$"; then
    printf '%s\n' \
      'workman dev: this installer is running inside Workman Dev.' \
      'Re-run it from Terminal.app (or another external shell) so stopping the Dev daemon' \
      'does not terminate the installer halfway through.' >&2
    exit 1
  fi
}

is_dev_daemon_process() {
  local pid=$1 command candidate
  command=$(process_command "$pid") || return 1
  [[ "$command" == *" --data-dir $data_dir" \
    || "$command" == *" --data-dir $data_dir "* ]] || return 1
  for candidate in \
    "$app_executable" \
    "$install_dir/workmand-dev" \
    "$bin_dir/workmand-dev"; do
    if [[ "$command" == "$candidate" || "$command" == "$candidate "* ]]; then
      return 0
    fi
  done
  return 1
}

is_dev_app_process() {
  local pid=$1 command
  command=$(process_command "$pid") || return 1
  [[ "$command" == "$app_executable" || "$command" == "$app_executable "* ]] || return 1
  ! is_dev_daemon_process "$pid"
}

collect_app_pids() {
  local pid command
  while read -r pid command; do
    [[ "$pid" =~ ^[0-9]+$ ]] || continue
    is_dev_app_process "$pid" && printf '%s\n' "$pid"
  done < <(/bin/ps -axo pid=,command=)
  return 0
}

collect_daemon_pids() {
  local pid command seen=" "
  pid=$(discovered_daemon_pid)
  if [[ "$pid" =~ ^[0-9]+$ ]] && is_dev_daemon_process "$pid"; then
    printf '%s\n' "$pid"
    seen="$seen$pid "
  fi
  while read -r pid command; do
    [[ "$pid" =~ ^[0-9]+$ ]] || continue
    case "$seen" in *" $pid "*) continue ;; esac
    if is_dev_daemon_process "$pid"; then
      printf '%s\n' "$pid"
      seen="$seen$pid "
    fi
  done < <(/bin/ps -axo pid=,command=)
  return 0
}

assert_not_ancestor_processes() {
  local pid
  for pid in "$@"; do
    if process_is_descendant_of "$pid" "$$"; then
      printf 'workman dev: refusing to stop ancestor process %s; run from Terminal.app\n' \
        "$pid" >&2
      exit 1
    fi
  done
}

target_process_is_alive() {
  local kind=$1 pid=$2
  process_is_alive "$pid" || return 1
  case "$kind" in
    app) is_dev_app_process "$pid" ;;
    daemon) is_dev_daemon_process "$pid" ;;
    *) return 1 ;;
  esac
}

terminate_processes() {
  local kind=$1 label=$2
  shift 2
  (( $# > 0 )) || return 0
  local pid attempt alive
  printf '  ▸ Stopping %s\n' "$label"
  for pid in "$@"; do
    target_process_is_alive "$kind" "$pid" \
      && /bin/kill -TERM "$pid" 2>/dev/null || true
  done
  attempt=0
  while (( attempt < 80 )); do
    alive=0
    for pid in "$@"; do
      target_process_is_alive "$kind" "$pid" && alive=1
    done
    (( alive == 0 )) && { printf '  ✓ Stopped %s\n' "$label"; return 0; }
    /bin/sleep 0.1
    attempt=$((attempt + 1))
  done
  printf '  ! %s did not exit gracefully; forcing it to stop\n' "$label"
  for pid in "$@"; do
    target_process_is_alive "$kind" "$pid" \
      && /bin/kill -KILL "$pid" 2>/dev/null || true
  done
  attempt=0
  while (( attempt < 20 )); do
    alive=0
    for pid in "$@"; do
      target_process_is_alive "$kind" "$pid" && alive=1
    done
    (( alive == 0 )) && { printf '  ✓ Stopped %s\n' "$label"; return 0; }
    /bin/sleep 0.1
    attempt=$((attempt + 1))
  done
  printf 'workman dev: could not stop %s\n' "$label" >&2
  exit 1
}

code_requirement() {
  local details
  details=$(codesign -d -r- "$1" 2>&1) || return 1
  /usr/bin/sed -n 's/^designated => //p' <<<"$details" | /usr/bin/sed -n '1p'
}

# A Workman-hosted shell will be torn down when the daemon exits, so fail before the long build.
ensure_external_installer_shell

signing_identity=${WORKMAN_DEV_SIGNING_IDENTITY:-}
if [[ -z "$signing_identity" ]]; then
  signing_identities=$(security find-identity -v -p codesigning 2>/dev/null || true)
  signing_identity=$(sed -nE 's/^[[:space:]]*[0-9]+\) [[:xdigit:]]+ "(Apple Development: [^"]+)"$/\1/p' <<<"$signing_identities" | sed -n '1p')
  if [[ -z "$signing_identity" ]]; then
    signing_identity=$(sed -nE 's/^[[:space:]]*[0-9]+\) [[:xdigit:]]+ "(Developer ID Application: [^"]+)"$/\1/p' <<<"$signing_identities" | sed -n '1p')
  fi
fi
if [[ -z "$signing_identity" ]]; then
  signing_identity=-
fi

printf '\n  Workman Dev · current-tree installer\n\n'
printf '  Identity  wrk-dev · workmand-dev · Workman Dev.app\n'
printf '  Bundle    %s\n' "$bundle_id"
printf '  Signing   %s\n' "$signing_identity"
printf '  Source    %s\n\n' "$repo_root"

printf '  ▸ Installing desktop dependencies\n'
npm --prefix "$repo_root/apps/desktop" install --no-audit --no-fund

printf '\n  ▸ Building current-tree CLI and daemon\n'
CARGO_TARGET_DIR="$build_dir" cargo build --manifest-path "$repo_root/Cargo.toml" \
  --release -p workman-cli -p workmand

printf '\n  ▸ Building Workman Dev.app\n'
(
  cd "$repo_root/apps/desktop"
  CARGO_TARGET_DIR="$build_dir" npm run tauri -- build --ci --no-sign --bundles app \
    --config src-tauri/tauri.dev.conf.json
)

release_dir="$build_dir/release"
source_app="$release_dir/bundle/macos/Workman Dev.app"
for source in "$release_dir/wrk" "$release_dir/workmand"; do
  [[ -x "$source" ]] || { printf 'workman dev: build did not produce %s\n' "$source" >&2; exit 1; }
done
[[ -d "$source_app" ]] || {
  printf 'workman dev: build did not produce %s\n' "$source_app" >&2
  exit 1
}
source_identifier=$(/usr/libexec/PlistBuddy -c 'Print :CFBundleIdentifier' \
  "$source_app/Contents/Info.plist")
if [[ "$source_identifier" != "$bundle_id" ]]; then
  printf 'workman dev: refusing bundle id %s; expected %s\n' \
    "$source_identifier" "$bundle_id" >&2
  exit 1
fi

# Tauri's --no-sign bundle is indexed by macOS before it is copied into ~/Applications.
# Give that source bundle the final stable identity first so Screen Recording never sees two
# Workman Dev apps with the same bundle id but incompatible code requirements.
codesign --force --deep --timestamp=none --options runtime --sign "$signing_identity" \
  --entitlements "$repo_root/apps/desktop/src-tauri/Entitlements.plist" "$source_app"
codesign --verify --deep --strict "$source_app"

app_parent=$(dirname "$app_path")
app_name=$(basename "$app_path")
app_stage="$app_parent/.$app_name.install-$$"
app_backup="$app_parent/.$app_name.replace-$$"
mkdir -p "$install_dir" "$bin_dir" "$app_parent"
for path in "$app_stage" "$app_backup"; do
  [[ ! -e "$path" ]] || {
    printf 'workman dev: temporary app path already exists: %s\n' "$path" >&2
    exit 1
  }
done
if [[ -e "$app_path" ]]; then
  installed_identifier=$(/usr/libexec/PlistBuddy -c 'Print :CFBundleIdentifier' \
    "$app_path/Contents/Info.plist" 2>/dev/null || true)
  if [[ "$installed_identifier" != "$bundle_id" ]]; then
    printf 'workman dev: refusing to replace %s with bundle id %s\n' \
      "$app_path" "${installed_identifier:-unknown}" >&2
    exit 1
  fi
fi

installed_requirement=$(code_requirement "$app_path" || true)
source_requirement=$(code_requirement "$source_app" || true)
permission_reset_reason=
if [[ "$reset_permissions" == 1 ]]; then
  permission_reset_reason='requested'
elif [[ "$reset_permissions" == auto ]]; then
  if [[ "$signing_identity" == - ]]; then
    permission_reset_reason='the app is ad hoc signed'
  elif [[ -z "$installed_requirement" ]]; then
    permission_reset_reason='there is no matching installed app identity'
  elif [[ -z "$source_requirement" || "$installed_requirement" != "$source_requirement" ]]; then
    permission_reset_reason='the app signing identity changed'
  fi
fi

ditto "$source_app" "$app_stage"
copied_identifier=$(/usr/libexec/PlistBuddy -c 'Print :CFBundleIdentifier' \
  "$app_stage/Contents/Info.plist")
if [[ "$copied_identifier" != "$bundle_id" ]]; then
  printf 'workman dev: staged app has unexpected bundle id %s\n' "$copied_identifier" >&2
  exit 1
fi
codesign --verify --deep --strict "$app_stage"

# Resolve every target before sending a signal, and never terminate an ancestor of this script.
app_pids=( $(collect_app_pids) )
daemon_pids=( $(collect_daemon_pids) )
if (( ${#app_pids[@]} > 0 )); then
  assert_not_ancestor_processes "${app_pids[@]}"
  terminate_processes app 'Workman Dev.app' "${app_pids[@]}"
fi
if (( ${#daemon_pids[@]} > 0 )); then
  assert_not_ancestor_processes "${daemon_pids[@]}"
  terminate_processes daemon 'the Workman Dev daemon and its sessions' "${daemon_pids[@]}"
fi

for program in wrk-dev workmand-dev; do
  case "$program" in
    wrk-dev) source="$release_dir/wrk" ;;
    workmand-dev) source="$release_dir/workmand" ;;
  esac
  temporary="$install_dir/.$program.install-$$"
  [[ ! -e "$temporary" ]] || {
    printf 'workman dev: temporary binary path already exists: %s\n' "$temporary" >&2
    exit 1
  }
  install -m 755 "$source" "$temporary"
  mv -f "$temporary" "$install_dir/$program"

  launcher="$bin_dir/$program"
  if [[ -e "$launcher" && ! -L "$launcher" ]]; then
    printf 'workman dev: refusing to replace non-symlink %s\n' "$launcher" >&2
    exit 1
  fi
  ln -sfn "$install_dir/$program" "$launcher"
  printf '  ✓ Linked %-14s %s\n' "$program" "$launcher"
done

if [[ -e "$app_path" ]]; then
  "$lsregister" -u "$app_path" >/dev/null 2>&1 || true
fi
if [[ -e "$app_path" ]]; then
  mv "$app_path" "$app_backup"
fi
if ! mv "$app_stage" "$app_path"; then
  if [[ -e "$app_backup" ]]; then
    mv "$app_backup" "$app_path"
  fi
  exit 1
fi
if [[ -e "$app_backup" ]]; then
  rm -rf -- "$app_backup"
fi
printf '  ✓ Installed %-14s %s\n' "Workman Dev.app" "$app_path"
"$lsregister" -u "$source_app" >/dev/null 2>&1 || true
"$lsregister" -f "$app_path" >/dev/null

if [[ -n "$permission_reset_reason" ]]; then
  printf '  ▸ Resetting macOS capture permissions (%s)\n' "$permission_reset_reason"
  /usr/bin/tccutil reset ScreenCapture "$bundle_id"
  /usr/bin/tccutil reset Microphone "$bundle_id"
  printf '  ✓ Screen Recording and Microphone access are ready to be requested again\n'
elif [[ "$reset_permissions" == auto ]]; then
  printf '  ✓ Preserved Screen Recording and Microphone access (signing identity unchanged)\n'
else
  printf '  ✓ Preserved macOS capture permissions as requested\n'
fi

dev_version=$("$install_dir/wrk-dev" --version)
case "$dev_version" in
  "workman-dev "*" (build "*")") ;;
  *) printf 'workman dev: installed wrk-dev reported an unexpected identity: %s\n' "$dev_version" >&2; exit 1 ;;
esac
update_notice=$("$install_dir/wrk-dev" update)
case "$update_notice" in
  *"rebuild from the current working tree with scripts/dev-install.sh"*) ;;
  *) printf 'workman dev: update isolation check failed: %s\n' "$update_notice" >&2; exit 1 ;;
esac

printf '\n  ✓ Workman Dev is installed side by side\n'
printf '    Version  %s\n' "$dev_version"
printf '    Binaries %s\n' "$install_dir"
printf '    App      %s\n' "$app_path"
printf '    Data     %s\n' "$data_dir"
printf '    Config   %s\n' "$data_dir/config.yml"
if [[ "$relaunch" == 1 ]]; then
  printf '\n  ▸ Opening Workman Dev\n'
  "$install_dir/wrk-dev" app
  printf '  ✓ Workman Dev is running\n\n'
else
  printf '\n  Workman Dev was not relaunched (--no-relaunch).\n'
  printf '  Run wrk-dev app when you are ready.\n\n'
fi
