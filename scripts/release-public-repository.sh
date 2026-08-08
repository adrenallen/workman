#!/usr/bin/env bash

# Reject release builds unless their canonical GitHub artifacts will be anonymously readable.
verify_public_release_repository() {
  local repository="$1"
  local gh_bin="${2:-gh}"
  local visibility

  if ! visibility="$("$gh_bin" api "repos/$repository" --jq '.visibility')"; then
    echo "could not verify GitHub repository visibility for $repository; refusing to release because Workman update and download URLs require public artifact access" >&2
    return 1
  fi

  if [[ "$visibility" != public ]]; then
    echo "GitHub repository $repository is ${visibility:-of unknown visibility}; refusing to release because Workman update and download URLs are unauthenticated and require a public repository" >&2
    return 1
  fi
}
