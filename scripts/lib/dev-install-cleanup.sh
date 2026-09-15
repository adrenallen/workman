#!/usr/bin/env bash

# Called only after installation succeeds.
workman_dev_cleanup() {
  local mode=$1 repo_root=$2 build_dir=$3
  shift 3
  local answer= protected candidate resolved_build

  case "$mode" in
    keep) return 0 ;;
    ask)
      if [[ ! -t 0 || ! -t 1 ]]; then
        printf '  Build cache kept. Use --cleanup to remove it after installation.\n'
        return 0
      fi
      printf '\n  Build output:\n'
      du -sh "$build_dir" "$repo_root/apps/desktop/dist" 2>/dev/null || true
      printf '  Cleanup removes the build cache; the next build will take longer.\n'
      printf '  Remove build output now? [y/N] '
      if ! IFS= read -r answer; then printf '\n'; fi
      case "$answer" in
        y|Y|yes|Yes|YES) ;;
        *) printf '  Build cache kept.\n'; return 0 ;;
      esac
      ;;
    clean) ;;
    *) printf 'workman dev: unknown cleanup mode: %s\n' "$mode" >&2; return 2 ;;
  esac

  # Resolve symlinks before cleaning a custom target directory. It must not contain
  # the source checkout, installed binaries/app, or the application's live data.
  for candidate in "$build_dir" "$repo_root/apps/desktop/dist"; do
    [[ -d "$candidate" ]] || continue
    candidate=$(cd -- "$candidate" && pwd -P) || return
    for protected in "$repo_root" "$@"; do
      [[ -d "$protected" ]] || continue
      protected=$(cd -- "$protected" && pwd -P) || return
      case "$protected/" in
        "${candidate%/}/"*)
          printf 'workman dev: cleanup refused: output directory %s contains %s\n' \
            "$candidate" "$protected" >&2
          return 1
          ;;
      esac
    done
  done
  if [[ -d "$build_dir" ]]; then
    resolved_build=$(cd -- "$build_dir" && pwd -P) || return
    printf '\n  ▸ Removing Rust build output from %s\n' "$resolved_build"
    cargo clean --manifest-path "$repo_root/Cargo.toml" --target-dir "$resolved_build" || return
  fi
  rm -rf -- "$repo_root/apps/desktop/dist" || return
  printf '  ✓ Build output removed. Workman Dev remains installed.\n'
}
