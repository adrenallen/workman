#!/usr/bin/env bash
set -euo pipefail

BUNDLE_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
BIN_DIR="$HOME/.local/bin"

for program in wrk workmand; do
  source_path="$BUNDLE_DIR/bin/$program"
  if [[ ! -x "$source_path" ]]; then
    echo "missing bundled executable: $source_path" >&2
    exit 1
  fi
done

read -r product version _ <<<"$("$BUNDLE_DIR/bin/wrk" --version)"
if [[ "$product" != "workman" || ! "$version" =~ ^[0-9]+\.[0-9]+\.[0-9]+(-[0-9A-Za-z.-]+)?(\+[0-9A-Za-z.-]+)?$ ]]; then
  echo "could not determine the bundled Workman version" >&2
  exit 1
fi

DIST_BIN_DIR="$HOME/.local/share/workman/dist/$version/bin"
mkdir -p "$BIN_DIR" "$DIST_BIN_DIR"
for program in wrk workmand; do
  source_path="$BUNDLE_DIR/bin/$program"
  install -m 755 "$source_path" "$DIST_BIN_DIR/$program"
  ln -sfn "$DIST_BIN_DIR/$program" "$BIN_DIR/$program"
  printf 'Installed %s and linked %s -> %s\n' \
    "$DIST_BIN_DIR/$program" "$BIN_DIR/$program" "$DIST_BIN_DIR/$program"
done

case ":${PATH:-}:" in
  *":$BIN_DIR:"*) ;;
  *)
    printf '\nAdd Workman to your PATH (put this in ~/.zshrc or ~/.bashrc):\n'
    # Print shell configuration for the user verbatim.
    # shellcheck disable=SC2016
    printf '  export PATH="$HOME/.local/bin:$PATH"\n'
    ;;
esac

if [[ -d "$BUNDLE_DIR/Workman.app" && -t 0 ]]; then
  printf '\nCopy the desktop app to /Applications? [y/N] '
  read -r answer
  case "$answer" in
    y|Y|yes|YES)
      if [[ -w /Applications ]]; then
        ditto "$BUNDLE_DIR/Workman.app" /Applications/Workman.app
      else
        echo "Administrator permission is needed to write to /Applications."
        sudo ditto "$BUNDLE_DIR/Workman.app" /Applications/Workman.app
      fi
      echo "Copied Workman.app to /Applications."
      ;;
  esac
fi

# The backticks are user-facing Markdown, not shell syntax.
# shellcheck disable=SC2016
printf '\nReady. Run `wrk --help`, or read GETTING-STARTED.md.\n'
