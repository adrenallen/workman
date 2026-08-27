# Workman technical details

This document collects implementation, configuration, development, and release information for
contributors and advanced users. For the product tour and normal installation flow, start with the
[main README](README.md).

## Architecture

Workman has four main components:

- `crates/workman-core` contains shared domain and service code.
- `crates/workmand` builds the headless `workmand` daemon.
- `crates/workman-cli` builds the `wrk` command-line client.
- `apps/desktop` contains the Tauri 2 desktop app and Svelte 5 frontend.

The daemon owns terminals, process state, persistence, and the local MCP and control APIs. The CLI
and desktop app are clients of that daemon. This lets processes continue when the desktop window
closes and gives agents, the CLI, and the UI a consistent view of the workspace.

The complete project specification, data model, protocol design, milestones, and design decisions
live in [PLAN.md](PLAN.md).

## Build and install from source

On macOS or Linux, run this from the repository root:

```sh
./install.sh
```

The installer builds the daemon, CLI, and desktop app in release mode and links them into
`~/.local/bin` without `sudo`. Re-run it after pulling updates. Then use `wrk` in a project
directory, `wrk app` to open the desktop workspace, or `wrk mcp-setup` to connect Claude Code.

On Windows, run the counterpart from PowerShell:

```powershell
powershell -ExecutionPolicy Bypass -File install.ps1
```

It installs the three binaries under `%LOCALAPPDATA%\Programs\Workman\bin`, adds that directory to
the user PATH unless `-NoPath` is given, and creates a Start Menu shortcut for the desktop app.

The Unix installer links `wrk`, `workmand`, and `workman-desktop`; it also creates a
`workman → wrk` convenience symlink unless `WORKMAN_INSTALL_ALIAS=0` is set. Obsolete pre-Workman
and `gbuild*` symlinks are removed only after the new binaries link successfully. Neither installer
stops a running daemon.

Build the Rust workspace without installing it with:

```sh
cargo build --workspace
```

The equivalent project shortcut is `just build`. Desktop-specific commands and environment
overrides are documented in [apps/desktop/README.md](apps/desktop/README.md).

## Data migration and repository configuration

On its first default-directory boot, Workman copies data from the most recent pre-Workman identity
when present, otherwise it falls back to `gbuild`. SQLite state and `config.yml` are preserved while
`daemon.json` is regenerated; source directories remain untouched.

Repository commands belong in `workman.yml`. The predecessor configuration filename and then
`gbuild.yml` remain readable with deprecation warnings. The product and GitHub repository are named
Workman, but an upgrade does not rename an existing checkout because running sessions may depend on
that path.

## Automation isolation

Automation must target a fresh, disposable Workman data directory instead of a developer's live
daemon. Set the safety guard in every harness or worker environment:

```sh
export WORKMAN_REQUIRE_EXPLICIT_DAEMON=1
export WORKMAN_DATA_DIR="$(mktemp -d /tmp/workman-automation.XXXXXX)"
wrk ps
```

`wrk --data-dir PATH ...` is the equivalent per-invocation form and takes precedence over
`WORKMAN_DATA_DIR`. With the guard set to `1`, daemon-targeting commands fail before discovery or
automatic startup unless a data-directory boundary is explicit. Supplying `--daemon` alone chooses
the executable but does not satisfy the isolation guard. Help, version, and update commands do not
target a daemon and remain available.

Each data directory keeps its MCP port and bearer credential in a private `mcp-endpoint.json` file.
The first daemon start selects a free loopback port; later starts reuse the port and token so
configured and already-running agent clients reconnect after an update. Stable, development, and
isolated automation identities remain independent. If a persisted port is occupied, the daemon
fails with the exact port and state path rather than silently changing the endpoint. Starting
`workmand --port PORT` once explicitly moves that identity to a chosen free port while preserving
its bearer token.

## Side-by-side development identity

Keep the stable release open while testing the current checkout with an isolated development
identity:

```sh
scripts/dev-install.sh
wrk-dev app
```

This installs `wrk-dev`, `workmand-dev`, and the visibly badged `Workman Dev.app` without replacing
`wrk`, `workmand`, or `Workman.app`. The development stack uses separate data, configuration,
daemon discovery, MCP registration, and a separate bundle identifier. Re-run the script after
source changes; `wrk-dev update` never installs over either identity. See
[GETTING-STARTED-DEV.md](GETTING-STARTED-DEV.md) for paths and the full workflow.

## Agent tool configuration

Agent commands belong to the active profile and are managed in desktop Settings. On the first boot
after upgrading to profile-aware storage, Workman imports the existing `agent_tools` block from
`workman/config.yml` into the Default profile. Later changes are stored in SQLite; the YAML block
remains a migration source and is not reapplied on every restart. Set `WORKMAN_CONFIG` to choose the
per-user file that also retains global update settings.

```yaml
agent_tools:
  - name: Codex
    command: codex --dangerously-bypass-approvals-and-sandbox
    tool_type: codex
    enabled: true
    # Appended to the original command when restarting a stopped agent.
    resume_args: resume {session_id}
    continue_args: resume --last
  - name: Custom agent
    command: /opt/agents/custom --interactive
    # tool_type is optional and inferred from the command executable.
```

Names are unique within a profile. Unknown `tool_type` values are accepted and use the generic
terminal-prompt attention detector. Resume behavior is preset-level and agent-agnostic:
`resume_args` must contain `{session_id}`, while `continue_args` is the working-directory fallback
used when no captured ID is available. Omitting both always starts that preset fresh. Workman
discovers session IDs passively from supported CLIs' own stores; it never probes or injects input
into a PTY.

## Profile storage and export behavior

Profiles switch the loaded project set, project order and selection, terminal shell override, agent
tool presets, and custom agent marks. Existing installs migrate into an active `Default` profile.

Canonical project data follows the project: processes and output history, todos, scratchpads,
worktree metadata, and project appearance are shared if the same path belongs to more than one
profile. Daemon and MCP credentials, update keys, notification history, UI-local theme and rail
preferences, and repository-local `workman.yml` files are global.

Use desktop **Settings → Profiles** or the CLI:

```sh
wrk profile list
wrk profile create "Demo recording" --empty
wrk profile switch 2 --stop-running
wrk profile export 1 ./main.workman-profile.json
wrk profile import ./main.workman-profile.json --name "Imported main"
wrk profile delete 2
```

A switch refuses to proceed while the outgoing profile has running work unless the desktop cascade
dialog or `--stop-running` confirms the exact stop set. The daemon is not restarted, so the MCP
endpoint and connected global sessions remain stable. A project-scoped session whose project is no
longer active fails project-scope checks explicitly.

Export archives contain paths, shell choice, agent presets, and custom agent PNGs. They never
contain endpoint tokens, update, download, or signing keys, process environments, output, todos, or
scratchpads. Presets that appear to embed credentials are rejected instead of exported.

## Project removal semantics

Desktop removal always opens a confirmation dialog for managed, adopted, and primary Git worktrees
as well as ordinary project folders. The default unregisters the project from the active profile
and keeps its files. Selecting **Also delete from my computer** permanently deletes the exact
displayed path and removes the canonical project from every profile.

The CLI exposes the same policy through `wrk project remove`; `wrk worktree remove` remains a
compatibility alias. Use `--delete-local` for disk deletion. Dirty or unpublished Git state and a
primary checkout with dependent linked worktrees require `--force --confirm TEXT`. Linked worktrees
use local `git worktree remove` and metadata pruning while preserving their local branch. Removal
never pushes, fetches, prunes remote refs, or deletes a remote branch. MCP callers use
`delete_project` with `delete_from_disk`, `force_dirty`, and `confirm_branch` for the same behavior.

## Scratchpad Markdown titles

A scratchpad's `name` is its canonical Markdown H1. When `scratchpad_write` or
`scratchpad_load_from_file` receives content whose first line is `# Title`, Workman stores `Title`
as the scratchpad name and removes that line from the body. Full and content reads return only the
body. Heading outlines, title-section reads, and `scratchpad_save_to_file` reconstruct the H1 from
the name. This normalization avoids keeping two title values that can drift apart.

## Release channels

Releases are built locally on an Apple silicon Mac and published as prereleases first. This is the
**latest** update channel for people who want each newly built version before promotion. The
default **stable** channel uses only promoted GitHub releases and ignores prereleases. Repository
release workflows are manually dispatched; tags do not trigger builds.

Choose a channel in desktop **Settings → Daemon**, or on the CLI:

```sh
wrk update --check --channel stable
wrk update --channel latest
```

The daemon persists the selected channel and weekly-update preference in `updates.json` inside the
Workman data directory.

## Cutting a release

Stamp the same version in the workspace, desktop package, and Tauri configuration; add a dated
Changelog section; commit and push `main`; then preview and run the complete local build:

```sh
scripts/release.sh --dry-run 0.1.0
scripts/release.sh 0.1.0
```

The command builds one portable archive per supported platform. `workman-macos-arm64.zip` contains
the app, CLI, daemon, installer, getting-started guide, and third-party notices. Each
`workman-linux-<arch>.tar.gz` contains the equivalent files with an experimental AppImage; matching
`.AppImage` and `.deb` files are emitted as standalone alternatives.

Artifacts and release notes are written under `release/vX.Y.Z` and checksummed before the tag or
GitHub prerelease is created. Re-running resumes from Cargo, npm, and container caches. After
installing and accepting the prerelease, promote it to stable with:

```sh
scripts/promote.sh vX.Y.Z
```

The manually dispatched Release workflow is an emergency build-only fallback and cannot publish a
release. Signing, notarization, credential setup, and renewal are documented in
[scripts/RELEASING.md](scripts/RELEASING.md). The public update host is documented separately in
[infra/update-host/README.md](infra/update-host/README.md).

## Contributor checks

Read [CONTRIBUTING.md](CONTRIBUTING.md) before opening a pull request. The standard validation set
is:

```sh
cargo fmt --all -- --check
cargo test --workspace --locked
(cd apps/desktop && npm ci && npm run check && npm run build)
(cd infra/update-host && npm ci && npm test && npm run check)
```

Documentation-only changes do not require a complete native application build. Security and
privacy expectations are in [SECURITY.md](SECURITY.md).
