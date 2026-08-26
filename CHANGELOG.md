# Changelog

All notable changes to Workman are recorded here.

## Unreleased

### Security and public distribution

- Remove the shared update key from shipped clients and make the Cloudflare-backed release
  manifest, download page, and artifacts public while accepting legacy clients that still send the
  retired header.
- Add private vulnerability-reporting guidance, secret scanning, dependency update automation,
  code ownership, contribution safeguards, and immutable GitHub Action pins.
- Keep routine GitHub Actions limited to secret scanning; desktop builds, tests, signing, and
  notarization remain part of the local release workflow.
- Update the transitive desktop build dependency `nanoid` to its patched release.
- Add public-source contribution guidance and generated third-party license notices to platform
  release archives.

### Projects and keyboard workflow

- Add configurable shortcuts for the first nine projects in rail order and for creating an agent,
  terminal, command, scratchpad, or todo in the current project; default to Command/Ctrl+1–9 and
  Command/Ctrl+N, persist changes locally, and expose assignments in Settings and the project rail.
- Restore recognizable OpenAI, Anthropic, DeepSeek, Grok, and Kimi marks across agent surfaces
  while preserving custom per-tool icons as the highest-priority display source.

## 0.1.11 - 2026-08-19

Workman 0.1.11 makes launched tools inherit the user's real shell environment, keeps automatic
sleep prevention tied to native agent state, and smooths project, terminal, and compact-window
workflows.

### Runtime reliability

- Resolve command and agent environments through an interactive login shell so runtimes managed
  by nvm, fnm, Volta, and asdf are available on `PATH`; runtime doctor now points to shell rc-file
  configuration when tools such as npm or Codex cannot be found.
- Evaluate automatic keep-awake natively from current agent state and require a verified power
  assertion, surviving hidden windows, persisted suppression edge cases, and daemon loss.
- Remove projects asynchronously so longer cleanup no longer reports a false daemon timeout.

### Projects and agent workflows

- Handle existing worktrees and branches gracefully during project creation, preserve truthful
  rollback behavior, and let stale operation rows be dismissed.
- Paste PNG clipboard images into Claude from both new-agent drafts and existing terminals, with
  durable attachment staging and previews.
- Keep menus, dropdowns, and selects reachable when the desktop window is small.

### Controls and platform support

- Make keep-awake a single-click, verified-armed control with an automatic mode, keep scratchpad
  outlines scrollable, and strengthen quick-prompt selection highlighting.
- Add Windows x86_64 release-archive tooling, installer polish, static CRT packaging, and update
  support, contributed by mleukering.

## 0.1.10 - 2026-08-18

Workman 0.1.10 brings creation and review work into project surfaces, makes updates and terminals
more resilient, and lays the groundwork for running Workman natively on Windows.

### Creation and project navigation

- Create agents, commands, and todos as persistent inline drafts instead of modal flows, with
  optimistic retry and keyboard navigation preserved.
- Set project titles while registering projects and creating, forking, or adopting worktrees.
- Redesign project rows with a compact meta strip, always-visible agent, terminal, and command
  indicators, state-toned click rosters, PR status last, and unclipped count badges.
- Add distinct worktree glyphs and icon badges, icon-only delayed project details, project-level
  Mark as read, and project identity on notification rows while removing other rail hover hints.

### Scratchpad review

- Add anchored scratchpad comments from user selections and agent MCP tools, including live
  re-anchoring, revision checks, permissions, resolution, and optional comment reads.
- Keep multi-line selection and comment highlights readable in light and dark themes.

### Updates, sessions, and terminals

- Show staged install progress in the update banner and Settings, then restart the replaced app
  and daemon automatically; command-line updates now request a daemon restart.
- Keep the macOS keep-awake assertion armed through daemon reconnects and sleep-sized gaps, with
  native watchdog recovery and clearer status.
- Give Kimi launches a credential-bound Workman MCP connection and more reliable prompt delivery.
- Import the native Terminal profile, recover rendering after hidden-view WebGL loss, and retain
  the WezTerm capability identity for truecolor and modified-key support.

### Agent spawning and platform foundations

- Add an optional model override to MCP and control agent spawning, compact template summaries,
  and dedicated spawning guidance.
- Add Windows runtime, ConPTY, PowerShell installation, path handling, and self-update groundwork
  for the daemon, CLI, and desktop app. Windows artifacts are not yet published by this pipeline.

## 0.1.9 - 2026-08-17

Workman 0.1.9 adds flexible workspace and agent workflows, expands project controls, and keeps
terminal input and agent launches responsive through slow runtime operations.

### Workspaces and projects

- Add switchable workspace profiles and project folders in the rail, plus desktop project timer
  controls.
- Make worktree creation and removal safer with explicit starting refs, guarded local deletion,
  and resilient cleanup.
- Show multiple pull requests per branch with merged PRs in purple, add command edit/removal
  controls, and wrap long todo and scratchpad titles.

### Agents and prompts

- Add reusable agent templates with a default agent and per-spawn overrides, a wider new-agent
  dialog, and a fully keyboard-driven Command-Shift-P quick-prompt palette with hotkey guidance.
- Add the Grok preset and state detection, Kimi and DeepSeek brand marks, branded agent picking,
  and automatic per-launch Workman MCP wiring for Kimi and Grok.
- Add desktop keep-awake lifecycle controls that can remain armed until agents become idle.

### Terminal experience

- Add a terminal context menu and Command-Up/Command-Down process cycling while preserving the
  shortcuts for terminal-level navigation.
- Keep terminal input responsive during agent spawns and daemon stalls, and correctly handle
  clipboard images sent to agents.

### Runtime and installation

- Scope MCP ownership to durable processes and fix hosted installer verification of durable paths.

## 0.1.8 - 2026-08-12

Workman 0.1.8 makes agent identity more expressive, multi-session work easier to manage, and
terminal/status updates more efficient under load.

### Desktop and session management

- Add agent brand marks and custom icon overrides across agent surfaces.
- Add Command/Control-click multi-selection with bulk actions for session management.

### Runtime and release infrastructure

- Drive attached terminal output and status snapshots through event-based invalidation.
- Park clean output spill workers when idle and harden the authenticated update host.

## 0.1.7 - 2026-08-11

Workman 0.1.7 makes active and resumed sessions feel immediate while reducing background work and
hardening process, update, and concurrent-agent behavior.

### Desktop and terminal experience

- Eliminate stopped-view flicker, paint idle-attached sessions immediately, and add project rail
  drag-and-drop reordering.
- Prevent terminal typing stalls, coalesce PTY renders, and serve range reads without taking full
  terminal snapshots.

### Runtime and lifecycle

- Cascade parent termination to child processes and coalesce dormant lifecycle work.
- Park idle timers and quiesce hidden sessions to reduce unnecessary CPU and battery use.
- Recover the CLI during app updates and make concurrent agent attribution and resume fallback
  reliable.

## 0.1.6 - 2026-08-10

Workman 0.1.6 makes release installs durable, so removing the original downloaded or extracted
bundle no longer breaks the installed command-line tools.

### Fixes

- Copy `wrk` and `workmand` into a version-owned directory before linking their launchers, making
  the bundled installer independent of its extraction location.
- Move command-line and Dock-launched updates through durable versioned installs while preserving
  and repairing discovered launchers, with regression coverage for installer and updater flows.
- Clarify in the macOS and Linux getting-started guides that the extracted folder can be deleted
  after installation.

## 0.1.5 - 2026-08-08

Workman 0.1.5 is the first signed and notarized release, removing the Gatekeeper workaround for
current downloads while preserving accurate guidance for older unsigned versions.

### Release operations

- Signed the Workman app, `wrk`, and `workmand` with a Developer ID certificate and hardened
  runtime, then added blocking notarization, stapling, and extracted-artifact verification before
  any release may publish.
- Added a publication-free signing test and documented certificate renewal, App Store Connect key
  rotation, rejection handling, and the trusted local release environment.
- Made the download and getting-started guidance version-aware so 0.1.5 and later use the signed
  launch path while 0.1.4 and earlier retain their required legacy Gatekeeper note.

## 0.1.4 - 2026-08-07

Workman 0.1.4 repairs in-app updates and makes attention and release surfaces more truthful.

### Fixes and release operations

- Fixed updates started from a Dock-launched app so they discover versioned installs and active
  launchers, refresh only a matching app identity, and guide app-only installs without a false
  missing-binary refusal.
- Added the Dock-launched app update hop to release verification, including matching-bundle,
  missing-CLI, and rejected-bundle regression cases.
- Rendered needs-input states as blue dots across agent surfaces, distinct from amber permission
  prompts, and removed obsolete pre-Workman aliases from public release notes and artifacts.
- Hardened local releases by rejecting private repositories and failing closed when a packaged
  desktop build points at a development URL.

## 0.1.3 - 2026-08-07

Workman 0.1.3 sharpens terminal and agent lifecycle behavior, adds native attention signals,
and expands collaboration tools while keeping local development and installed releases isolated.

### Highlights

- Added native notifications, a live Dock unread badge, and first-class needs-input, assignment,
  and mention events, with focus-aware delivery and direct navigation back to the relevant work.
- Made relaunched agents, terminals, and commands fit their PTY geometry on first paint; added
  conversation resume, clearer start and restart controls, and accurate idle, working, waiting,
  completed, failed, and stopped activity semantics.
- Improved terminal fidelity with modern keyboard protocols, natural editing shortcuts, themes,
  file and image transfer support, durable replay, and more reliable input handling.
- Expanded project and document workflows with a flat reorderable rail, durable pane state,
  optimistic navigation, richer todo and scratchpad documents, blockers, claimants, and mentions.

### Runtime and operations

- Added a side-by-side Workman Dev identity and current-tree installer whose app, daemon, CLI,
  configuration, and data remain separate from the stable installation.
- Hardened MCP agent identities and project boundaries, installer routing, authenticated updates,
  release retention, and compatibility endpoints for older installed updaters.

## 0.1.2 - 2026-08-06

Workman 0.1.2 makes the desktop easier to read and operate, strengthens isolated agent launches,
and completes the authenticated self-hosted release path from download through promotion.

### Highlights

- Added a persistent notification center for completed agents, compact project-rail behavior,
  section overview pages, safer dialogs, clearer todo states, and a searchable scratchpad browser.
- Added project appearance controls with custom icons and images throughout the rail and overview
  surfaces, plus the Workman mark in the app chrome, About screen, Dock bundle, README, and lander.
- Made the agent registry editable, reorderable, restart-persistent, and truthful about runtime and
  MCP availability; supported agents receive isolated per-launch Workman MCP wiring and deep checks.
- Made worktree import an explicit user action so linked worktrees never interrupt startup with an
  automatic prompt, while retaining PR status and repository-aware project navigation.

### Release operations

- Added Cloudflare R2-backed stable/latest release manifests, authenticated artifact downloads,
  keyed POSIX installer delivery with explicit channel selection, and updater key support with
  honest authorization failures.
- Kept GitHub prereleases as the compatibility bridge for older updaters while local release and
  promotion scripts publish the complete macOS/Linux artifact set without triggering Actions.
- Updated `wrk app` to launch the installed macOS bundle through LaunchServices so the branded
  Workman Dock icon is used instead of a generic executable icon.

## 0.1.1 - 2026-08-05

The first Workman release turns the original preview into a more complete work manager, with a
precise desktop design system, first-class Git worktrees, durable terminal history, and a
migration-safe rename. Existing v0.1.0 preview installs can update directly to this release.

### Highlights

- Renamed the preview product to Workman: the terminal command is now `wrk`, the daemon is
  `workmand`, and runtime configuration uses `WORKMAN_*` with a `workman` MCP identity.
- Added a non-destructive first-run migration chain that prefers existing preview state and falls
  back to gbuild, while `workman.yml` retains warned read compatibility with predecessor
  configuration.
- Added the one-release updater bridge and transitional pre-Workman assets required for real v0.1.0
  clients to discover, checksum, and install the renamed Workman binaries.
- Rebuilt the desktop on shadcn-svelte, bits-ui, Tailwind, semantic tokens, shared primitives, and
  a legible dense type scale; added native macOS menus and a full Settings About/Updates section.
- Added complete Git worktree management: repository grouping, create/adopt/remove, exact-HEAD
  fork-again, Laravel Herd URLs, safe ignored `.env` porting, cached GitHub PR/check/merge status,
  Runtime Doctor health checks, context actions, and Cmd+K flows.
- Persisted bounded raw terminal and agent output across daemon restarts, replaying it through the
  server-side terminal emulator so the UI, `wrk logs`, search, and MCP output retain history.
- Unified the Workman app, `wrk`, `workmand`, installer, and getting-started guide into one archive
  per platform, with Linux AppImage and Debian alternatives clearly marked experimental.

### Release operations

- Moved cross-platform release builds and prerelease publication to one local command, retaining
  GitHub Actions only as a manual build-only fallback.
- Added stable and latest update channels, prerelease-first publishing, explicit promotion, a
  shared dist profile, and release-build caches.

## 0.1.0 - 2026-08-05

The first native preview is a Solo-style workspace for running coding agents beside a development
stack. A durable Rust daemon owns terminals and coordination state while the CLI and Tauri desktop
app remain reconnectable clients.

### Highlights

- Project and PTY process management with server-rendered terminal history, readiness checks,
  lifecycle automation, trust-gated repository commands, and live resource status.
- A local MCP server for projects, processes, todos, scratchpads, locks, timers, agent spawning,
  attention states, and multi-agent coordination.
- A desktop workspace with terminal panels, process trees, WYSIWYG scratchpads, settings, agent
  runtime management, and Runtime Doctor health/configuration checks.
- Per-launch MCP routing for supported agent CLIs, dialog safety, timer wake-ups, and persistent
  state across daemon and UI restarts.
- First-run migration from the former gbuild data directory, plus a warned `gbuild.yml` read
  fallback during the rename transition.

### Known gaps

- Linux support is experimental and has not yet received the same end-to-end testing as macOS.
- Distributed desktop and CLI binaries are not code-signed or notarized yet.
- The MCP implementation intentionally remains on the rmcp 1.x line while its APIs stabilize.
