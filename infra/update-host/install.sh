#!/bin/sh
set -eu

usage() {
  cat <<'EOF'
Install the current stable Workman release.

Usage:
  curl -fsSL https://workman.userdefined.io/install.sh | \
    sh -s -- --key <download-key>

  curl -fsSL https://workman.userdefined.io/install.sh | \
    WORKMAN_KEY=<download-key> sh

Options:
  --key <download-key>  Shared Workman download key (overrides WORKMAN_KEY)
  --help, -h            Show this help

Environment:
  WORKMAN_KEY          Shared Workman download key
  WORKMAN_INSTALL_DIR  Versioned bundle destination
EOF
}

download_key="${WORKMAN_KEY:-}"
while [ "$#" -gt 0 ]; do
  case "$1" in
    --key)
      if [ "$#" -lt 2 ]; then
        echo "--key requires a value" >&2
        usage >&2
        exit 2
      fi
      download_key="$2"
      shift 2
      ;;
    --key=*)
      download_key="${1#--key=}"
      shift
      ;;
    --help|-h)
      usage
      exit 0
      ;;
    *)
      echo "unknown option: $1" >&2
      usage >&2
      exit 2
      ;;
  esac
done

if [ -z "$download_key" ]; then
  echo "a Workman download key is required; pass --key or set WORKMAN_KEY" >&2
  usage >&2
  exit 2
fi

for command in curl python3; do
  if ! command -v "$command" >/dev/null 2>&1; then
    echo "required command not found: $command" >&2
    exit 1
  fi
done

case "$(uname -s)-$(uname -m)" in
  Darwin-arm64) target="macos-arm64"; archive_kind="zip" ;;
  Linux-x86_64) target="linux-x86_64"; archive_kind="tar" ;;
  Linux-aarch64|Linux-arm64) target="linux-arm64"; archive_kind="tar" ;;
  *)
    echo "Workman does not publish a bundle for $(uname -s) $(uname -m)" >&2
    exit 1
    ;;
esac

base_url="${WORKMAN_UPDATE_BASE_URL:-https://workman.userdefined.io}"
temporary_dir="$(mktemp -d "${TMPDIR:-/tmp}/workman-install.XXXXXX")"
trap 'rm -rf "$temporary_dir"' 0
manifest_path="$temporary_dir/releases.json"
release_metadata_path="$temporary_dir/release-metadata"
archive_path="$temporary_dir/release"
stage_dir="$temporary_dir/stage"
mkdir -p "$stage_dir"

fetch_with_key() {
  curl --fail --silent --show-error --location --retry 3 \
    --header "Authorization: Bearer $download_key" "$@"
}

echo "Reading the Workman stable channel..."
fetch_with_key "$base_url/releases.json" --output "$manifest_path"

python3 - "$manifest_path" "$target" "$base_url" > "$release_metadata_path" <<'PY'
import json
import re
import sys
from urllib.parse import urlparse

manifest_path, target, base_url = sys.argv[1:]
with open(manifest_path, encoding="utf-8") as source:
    manifest = json.load(source)

release = manifest["channels"]["stable"]
version = release["version"]
if re.fullmatch(r"\d+\.\d+\.\d+", version) is None:
    raise SystemExit("the update server returned an invalid stable version")

asset = next((candidate for candidate in release["assets"] if candidate["target"] == target), None)
if asset is None:
    raise SystemExit(f"the stable release has no {target} bundle")

sha256 = asset["sha256"]
if re.fullmatch(r"[a-f0-9]{64}", sha256) is None:
    raise SystemExit("the update server returned an invalid artifact checksum")

artifact_url = urlparse(asset["url"])
server_url = urlparse(base_url)
if (
    artifact_url.scheme != server_url.scheme
    or artifact_url.netloc != server_url.netloc
    or not artifact_url.path.startswith(f"/versions/{version}/")
):
    raise SystemExit("the update server returned an untrusted artifact URL")

print(version)
print(asset["url"])
print(sha256)
PY

version=
artifact_url=
expected_sha256=
{
  IFS= read -r version || :
  IFS= read -r artifact_url || :
  IFS= read -r expected_sha256 || :
} < "$release_metadata_path"

if [ -z "$version" ] || [ -z "$artifact_url" ] || [ -z "$expected_sha256" ]; then
  echo "the update server returned incomplete release metadata" >&2
  exit 1
fi

echo "Downloading Workman $version for $target..."
fetch_with_key "$artifact_url" --output "$archive_path"

if command -v sha256sum >/dev/null 2>&1; then
  actual_sha256="$(sha256sum "$archive_path" | awk '{print $1}')"
else
  actual_sha256="$(shasum -a 256 "$archive_path" | awk '{print $1}')"
fi
if [ "$actual_sha256" != "$expected_sha256" ]; then
  echo "download checksum mismatch: expected $expected_sha256, got $actual_sha256" >&2
  exit 1
fi

case "$archive_kind" in
  zip)
    command -v unzip >/dev/null 2>&1 || { echo "required command not found: unzip" >&2; exit 1; }
    unzip -q "$archive_path" -d "$stage_dir"
    ;;
  tar)
    tar -xzf "$archive_path" -C "$stage_dir"
    ;;
esac

install_dir="${WORKMAN_INSTALL_DIR:-$HOME/.local/share/workman/$version}"
mkdir -p "$install_dir"
cp -R "$stage_dir/." "$install_dir/"
if [ ! -f "$install_dir/install.sh" ]; then
  echo "the Workman bundle does not contain install.sh" >&2
  exit 1
fi

bash "$install_dir/install.sh"
printf '\nInstalled Workman %s from a checksum-verified bundle at %s.\n' "$version" "$install_dir"
