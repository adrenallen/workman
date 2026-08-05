# Workman

Workman is a native terminal workspace for running AI coding agents alongside a development
stack. A headless Rust daemon will own terminals, process state, persistence, and the local MCP
and control APIs; the CLI and desktop app will be thin clients.

The full project specification, architecture, milestones, and design decisions live in
[PLAN.md](PLAN.md).

## Install

```sh
./install.sh
```

This builds the daemon, CLI, and desktop app in release mode and links them into
`~/.local/bin` without sudo. Re-run it after pulling updates. Then run `wrk` in any
project directory, `wrk app` for the desktop workspace, or `wrk mcp-setup` for Claude Code.
The installer links `wrk`, `workmand`, and `workman-desktop`; it also creates a `workman → wrk`
convenience symlink unless `WORKMAN_INSTALL_ALIAS=0` is set. Obsolete `awm*` and `gbuild*`
symlinks are removed only after the new binaries link successfully, and running daemons are never
stopped.

On its first default-directory boot, Workman copies the `awm` data directory when present,
otherwise it falls back to `gbuild`. SQLite state and `config.yml` are preserved while
`daemon.json` is regenerated; both source directories remain untouched. Repository commands
belong in `workman.yml`; `awm.yml` and then `gbuild.yml` remain readable with deprecation warnings.

## Workspace

- `crates/workman-core` — shared domain and service code
- `crates/workmand` — headless daemon binary
- `crates/workman-cli` — `wrk` command-line client
- `apps/desktop` — Tauri 2 desktop app

Run `cargo build --workspace` or `just build` to build the Rust workspace.

## Release channels

Releases are built locally on an Apple silicon Mac and published as prereleases first. This is
the **latest** update channel, intended for people who want each newly built version before it
is promoted. The default **stable** channel uses only promoted GitHub releases and therefore
ignores prereleases. Tags do not trigger GitHub Actions; both repository workflows are manual.

Choose a channel in Settings → Daemon, or on the CLI:

```sh
wrk update --check --channel stable
wrk update --channel latest
```

The daemon persists the selected channel with its weekly-update preference in `updates.json`
inside the Workman data directory.

### Cutting a release

Stamp the same version in the workspace, desktop package, and Tauri config; add a dated
CHANGELOG section; commit and push `main`; then preview the complete local build:

```sh
scripts/release.sh --dry-run 0.1.0
scripts/release.sh 0.1.0
```

The command builds one portable archive per platform. `workman-macos-arm64.zip` contains the app,
CLI, daemon, installer, and a human getting-started guide. Each `workman-linux-<arch>.tar.gz`
contains the same pieces with an experimental AppImage; matching `.AppImage` and `.deb` files are
also emitted as standalone alternatives.
The `awm-*.tar.gz` and `awm-desktop-*` files are temporary compatibility aliases for the updater
shipped in v0.1.0, not downloads for new users.

Artifacts and release notes are written under `release/vX.Y.Z` and checksummed before the tag
or GitHub prerelease is created. Re-running is safe and resumes from Cargo, npm, and container
caches. After installing and accepting the prerelease, promote it to stable with
`scripts/promote.sh vX.Y.Z`.

The manually dispatched Release workflow remains only as an emergency build-only fallback; it
cannot publish a release.

The checkout itself may still be located at `/Users/g/Code/gbuild`. The product and GitHub
repository are named Workman, but that live working-directory path is intentionally not moved by
the rename.

## User agent tools

Agent commands can be managed in `workman/config.yml` beneath the platform config directory
(`~/Library/Application Support/workman/config.yml` on macOS, or
`${XDG_CONFIG_HOME:-~/.config}/workman/config.yml` on Linux). Set `WORKMAN_CONFIG` to use a
different file. The daemon reconciles this file on startup; file-backed entries are visible but
read-only in the desktop settings, while tools created in the UI remain in the database.

```yaml
agent_tools:
  - name: Codex
    command: codex --dangerously-bypass-approvals-and-sandbox
    tool_type: codex
    enabled: true
  - name: Custom agent
    command: /opt/agents/custom --interactive
    # tool_type is optional and inferred from the command executable.
```

Names are the stable identity for file-backed entries. Removing one from the file removes that
managed entry at the next daemon restart. Unknown `tool_type` values are accepted and use the
generic terminal-prompt attention detector.

## Scratchpad Markdown Titles

A scratchpad's `name` is its canonical Markdown H1. When `scratchpad_write` or
`scratchpad_load_from_file` receives content whose first line is `# Title`, Workman stores
`Title` as the scratchpad name and removes that line from the body. Full/content reads return
the body only; heading outlines, title-section reads, and `scratchpad_save_to_file` reconstruct
the H1 from the name. This normalization avoids keeping two title values that can drift apart.
