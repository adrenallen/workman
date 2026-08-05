# awm

awm is a native terminal workspace for running AI coding agents alongside a development
stack. A headless Rust daemon will own terminals, process state, persistence, and the local MCP
and control APIs; the CLI and desktop app will be thin clients.

The full project specification, architecture, milestones, and design decisions live in
[PLAN.md](PLAN.md).

## Install

```sh
./install.sh
```

This builds the daemon, CLI, and desktop app in release mode and links them into
`~/.local/bin` without sudo. Re-run it after pulling updates. Then run `awm` in any
project directory, `awm app` for the desktop workspace, or `awm mcp-setup` for Claude Code.
The installer removes obsolete `gbuild`, `gbuildd`, and `gbuild-desktop` symlinks after the
new binaries have linked successfully; it never stops an already-running legacy daemon.

On its first default-directory boot, awm copies the pre-rename gbuild data directory into the
new awm directory, preserving SQLite state and `config.yml` while regenerating `daemon.json`.
The old directory is left untouched. Repository commands belong in `awm.yml`; a legacy
`gbuild.yml` remains readable for this release and emits a deprecation warning.

## Workspace

- `crates/awm-core` — shared domain and service code
- `crates/awmd` — headless daemon binary
- `crates/awm` — `awm` command-line client
- `apps/desktop` — Tauri 2 desktop app

Run `cargo build --workspace` or `just build` to build the Rust workspace.

## Release channels

Releases are built locally on an Apple silicon Mac and published as prereleases first. This is
the **latest** update channel, intended for people who want each newly built version before it
is promoted. The default **stable** channel uses only promoted GitHub releases and therefore
ignores prereleases. Tags do not trigger GitHub Actions; both repository workflows are manual.

Choose a channel in Settings → Daemon, or on the CLI:

```sh
awm update --check --channel stable
awm update --channel latest
```

The daemon persists the selected channel with its weekly-update preference in `updates.json`
inside the awm data directory.

### Cutting a release

Stamp the same version in the workspace, desktop package, and Tauri config; add a dated
CHANGELOG section; commit and push `main`; then preview the complete local build:

```sh
scripts/release.sh --dry-run 0.1.0
scripts/release.sh 0.1.0
```

The command builds the native macOS binaries and unsigned app, static musl Linux binaries for
x86_64 and arm64, and—when Docker or OrbStack is available—experimental Linux AppImage and
Debian bundles. It writes checksummed artifacts under `release/vX.Y.Z`, creates and pushes the
tag only after every required artifact verifies, and creates a GitHub prerelease. Re-running is
safe and resumes from Cargo, npm, and container caches. After installing and accepting the
prerelease, promote it to stable with `scripts/promote.sh vX.Y.Z`.

The manually dispatched Release workflow remains only as an emergency build-only fallback; it
cannot publish a release.

The checkout itself may still be located at `/Users/g/Code/gbuild`. The product and GitHub
repository are named awm, but that live working-directory path is intentionally not moved by
the rename.

## User agent tools

Agent commands can be managed in `awm/config.yml` beneath the platform config directory
(`~/Library/Application Support/awm/config.yml` on macOS, or
`${XDG_CONFIG_HOME:-~/.config}/awm/config.yml` on Linux). Set `AWM_CONFIG` to use a
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
`scratchpad_load_from_file` receives content whose first line is `# Title`, awm stores
`Title` as the scratchpad name and removes that line from the body. Full/content reads return
the body only; heading outlines, title-section reads, and `scratchpad_save_to_file` reconstruct
the H1 from the name. This normalization avoids keeping two title values that can drift apart.
