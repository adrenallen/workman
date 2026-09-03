#!/usr/bin/env bash

set -euo pipefail

usage() {
  cat <<'EOF'
Build and install the current Workman working tree as an isolated development identity.

Usage: scripts/dev-install.sh

Installs:
  ~/.local/bin/wrk-dev
  ~/.local/bin/workmand-dev
  ~/Applications/Workman Dev.app  (bundle id com.workman.dev)

Environment overrides:
  WORKMAN_DEV_BIN_DIR       Launcher directory (default: ~/.local/bin)
  WORKMAN_DEV_INSTALL_DIR   Private binary directory (default: ~/.local/share/workman-dev)
  WORKMAN_DEV_APP_PATH      App destination (default: ~/Applications/Workman Dev.app)
  WORKMAN_DEV_BUILD_DIR     Cargo/Tauri target directory (default: target)
  WORKMAN_DEV_SIGNING_IDENTITY
                            macOS signing identity (default: first Apple Development
                            or Developer ID Application identity; use - for ad hoc)
EOF
}

case "${1:-}" in
  --help|-h) usage; exit 0 ;;
  "") ;;
  *) printf 'workman dev: unknown option: %s\n' "$1" >&2; usage >&2; exit 2 ;;
esac
if (( $# > 1 )); then
  printf 'workman dev: no positional arguments are accepted\n' >&2
  usage >&2
  exit 2
fi

repo_root=$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")/.." && pwd -P)
install_home=${HOME:?HOME must be set}
bin_dir=${WORKMAN_DEV_BIN_DIR:-"$install_home/.local/bin"}
install_dir=${WORKMAN_DEV_INSTALL_DIR:-"$install_home/.local/share/workman-dev"}
app_path=${WORKMAN_DEV_APP_PATH:-"$install_home/Applications/Workman Dev.app"}
build_dir=${WORKMAN_DEV_BUILD_DIR:-"$repo_root/target"}
dev_config="$repo_root/apps/desktop/src-tauri/tauri.dev.conf.json"

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
for required in cargo npm ditto codesign security /usr/libexec/PlistBuddy; do
  if [[ "$required" == /* ]]; then
    [[ -x "$required" ]] || { printf 'workman dev: required tool not found: %s\n' "$required" >&2; exit 1; }
  elif ! command -v "$required" >/dev/null 2>&1; then
    printf 'workman dev: required tool not found: %s\n' "$required" >&2
    exit 1
  fi
done

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
printf '  Bundle    com.workman.dev\n'
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
if [[ "$source_identifier" != com.workman.dev ]]; then
  printf 'workman dev: refusing bundle id %s; expected com.workman.dev\n' \
    "$source_identifier" >&2
  exit 1
fi

mkdir -p "$install_dir" "$bin_dir" "$(dirname "$app_path")"
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

app_parent=$(dirname "$app_path")
app_name=$(basename "$app_path")
app_stage="$app_parent/.$app_name.install-$$"
app_backup="$app_parent/.$app_name.replace-$$"
for path in "$app_stage" "$app_backup"; do
  [[ ! -e "$path" ]] || {
    printf 'workman dev: temporary app path already exists: %s\n' "$path" >&2
    exit 1
  }
done
if [[ -e "$app_path" ]]; then
  installed_identifier=$(/usr/libexec/PlistBuddy -c 'Print :CFBundleIdentifier' \
    "$app_path/Contents/Info.plist" 2>/dev/null || true)
  if [[ "$installed_identifier" != com.workman.dev ]]; then
    printf 'workman dev: refusing to replace %s with bundle id %s\n' \
      "$app_path" "${installed_identifier:-unknown}" >&2
    exit 1
  fi
fi

ditto "$source_app" "$app_stage"
copied_identifier=$(/usr/libexec/PlistBuddy -c 'Print :CFBundleIdentifier' \
  "$app_stage/Contents/Info.plist")
if [[ "$copied_identifier" != com.workman.dev ]]; then
  printf 'workman dev: staged app has unexpected bundle id %s\n' "$copied_identifier" >&2
  exit 1
fi
codesign --force --deep --timestamp=none --options runtime --sign "$signing_identity" \
  --entitlements "$repo_root/apps/desktop/src-tauri/Entitlements.plist" "$app_stage"
codesign --verify --deep --strict "$app_stage"
if [[ "$signing_identity" == - ]]; then
  printf '  ! No Apple signing identity was found; macOS privacy access may need to be granted again after each rebuild.\n'
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
printf '    Data     %s\n' "$install_home/Library/Application Support/workman-dev"
printf '    Config   %s\n' "$install_home/Library/Application Support/workman-dev/config.yml"
printf '\n  Next:\n'
printf '    wrk-dev              # use the isolated dev daemon\n'
printf '    wrk-dev app          # open the badged Workman Dev.app\n'
printf '    wrk-dev mcp-setup    # print workman-dev MCP registration\n'
printf '    scripts/dev-install.sh  # rebuild after source changes\n\n'
