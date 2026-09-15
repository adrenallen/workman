#!/usr/bin/env bash

# Use the installed CLI so launching also works after build output has been removed.
workman_dev_relaunch() {
  local mode=$1 install_dir=$2 answer=
  case "$mode" in
    ask)
      mode=0
      if [[ -t 0 && -t 1 ]]; then
        printf '\n  Open Workman Dev now (wrk-dev app)? [Y/n] '
        if IFS= read -r answer; then
          case "$answer" in
            ''|y|Y|yes|Yes|YES) mode=1 ;;
          esac
        else
          printf '\n'
        fi
      fi
      ;;
    0|1) ;;
    *) printf 'workman dev: unknown relaunch mode: %s\n' "$mode" >&2; return 2 ;;
  esac

  if [[ "$mode" == 1 ]]; then
    printf '\n  ▸ Opening Workman Dev\n'
    "$install_dir/wrk-dev" app || return
    printf '  ✓ Workman Dev is running\n\n'
  else
    printf '\n  Workman Dev was left closed.\n'
    printf '  Run wrk-dev app when you are ready, or use --relaunch on your next install.\n\n'
  fi
}
