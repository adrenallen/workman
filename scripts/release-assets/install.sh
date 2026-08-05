#!/usr/bin/env bash
set -euo pipefail

BUNDLE_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
BIN_DIR="$HOME/.local/bin"

mkdir -p "$BIN_DIR"
for program in awm awmd; do
  source_path="$BUNDLE_DIR/bin/$program"
  if [[ ! -x "$source_path" ]]; then
    echo "missing bundled executable: $source_path" >&2
    exit 1
  fi
  ln -sfn "$source_path" "$BIN_DIR/$program"
  printf 'Linked %s -> %s\n' "$BIN_DIR/$program" "$source_path"
done

case ":${PATH:-}:" in
  *":$BIN_DIR:"*) ;;
  *)
    printf '\nAdd awm to your PATH (put this in ~/.zshrc or ~/.bashrc):\n'
    printf '  export PATH="$HOME/.local/bin:$PATH"\n'
    ;;
esac

if [[ -d "$BUNDLE_DIR/awm.app" && -t 0 ]]; then
  printf '\nCopy the desktop app to /Applications? [y/N] '
  read -r answer
  case "$answer" in
    y|Y|yes|YES)
      if [[ -w /Applications ]]; then
        ditto "$BUNDLE_DIR/awm.app" /Applications/awm.app
      else
        echo "Administrator permission is needed to write to /Applications."
        sudo ditto "$BUNDLE_DIR/awm.app" /Applications/awm.app
      fi
      echo "Copied awm.app to /Applications."
      ;;
  esac
fi

printf '\nReady. Run `awm --help`, or read GETTING-STARTED.md.\n'
