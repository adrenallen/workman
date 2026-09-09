# Changelog

All notable changes to Workman are recorded here.

## Unreleased

## 0.1.15 - 2026-09-08

Workman 0.1.15 brings a clearer workspace, easier scratchpad editing, recoverable agent prompts,
and more control over feedback and notifications.

### Workspace and navigation

- Bring restrained blue and purple accents from the Workman logo into navigation and chrome,
  with readable selected sidebar text in both themes. Remove the total Projects count while
  retaining folder counts.
- Introduce a full-logo first-project welcome screen with “Let’s get to work” and an Add a project
  action. Reorganize Settings into grouped navigation with larger copy, roomier rows, and a
  compact section selector when space is limited.
- Clarify the New Agent flow with distinct template and model choices while preserving template
  overrides, model settings, prompt history, attachments, and instructions.
- Show terminal name overrides in the sidebar and allow double-click renaming of project items.
  Show the actual branch name instead of an ambiguous HEAD label in branch selection.
- Remember trusted exact commands per project and use a theme-aware command review dialog.
- Add a checked Notify when idle context-menu option for agents, terminals, and commands.

### Scratchpads and process controls

- Resume the focused stopped agent, terminal, or command with Control+R. Configure or clear
  the shortcut in Settings → Hotkeys; running processes retain their normal Control+R input.

- Keep scratchpad scroll, cursor, and unfinished edits when switching panes. Code fences now
  render as editable code blocks with Copy and Edit controls; comment markers open threads
  without hijacking text clicks. Open scratchpads in the configured editor and sync saved
  Markdown back with revision conflict handling.
- Hold Cmd+` (Ctrl+` outside macOS) to cycle recent tabs with repeated backquote presses or
  the arrow keys, then release the modifier to switch. The list contains up to ten available
  tabs visited within the last ten minutes.
- Make scratchpad text selections visible with a blue highlight, including inside code blocks.

### Updates and storage

- Repair self-update on Linux desktops and Windows. AppImage and package installs run the desktop
  as their daemon and have no wrk/workmand pair beside it; updates now install the command-line
  tools into the versioned layout, replace a writable AppImage in place and relaunch it, and give
  package installs an honest instruction instead of failing. Windows installs update
  `Programs\Workman\bin` in place and relaunch the replaced desktop. A daemon started by `wrk`
  also refreshes the AppImage the desktop was launched from. `wrk update` no longer fails with
  "key must be a string" against a running daemon when no update key is configured. Releases
  without a package for the current platform are shown as download-only instead of offering an
  update that cannot install, and the release pipeline, manifest, and download page carry the
  Windows archive whose bundled `install.ps1` now installs prebuilt binaries.
- Run bounded storage maintenance at startup and hourly: expire old notifications, completed
  one-time timers, and abandoned leases; clean orphan output and feedback media; and reuse or
  reclaim free SQLite pages. Removing a project from its last profile clears its Workman data.
  Shared projects retain their data, user documents do not expire, and agent deletion is unchanged.

### Notifications and dictation

- Keep speech-model verification off the UI thread and recover abandoned dictation recordings
  safely after a crash. Keep live notifications responsive during audio previews, repair damaged
  volume copies, bound their cache, and reuse Linux notification connections. Refresh process
  state before reconnect alerts and clear project-ready records once per work cycle.

- Keep notification delivery active in the dev window configuration, and run the five-second
  notification test in native code so switching apps cannot pause its countdown. Restore its
  result when returning to Settings. Add saved volume control for Doom and custom WAVs on
  macOS/Linux; previews and real notifications use the same volume without changing the original.

- Add a speaker button beside the notification sound dropdown for immediate audio previews,
  including Doom, custom WAVs, and the system sound, without sending a notification banner.

- Check application activation as well as window/WebView focus before suppressing computer alerts.
  Keep a selected agent's blue unread dot while switched away, and clear it and matching OS alerts
  after three continuous seconds of viewing. Add a five-second notification test in Settings and
  a system-settings shortcut when macOS allows banners but has notification sounds disabled.

- Add a Notification mode selector for all agents, top-level agents, or one project-ready alert
  after every agent (including children) stops working. Wait through brief handoffs and notify once
  per work cycle. Migrate the previous overlapping switches into the selected mode.
- Bundle the Doom item pickup sound in the notification sound dropdown. Keep System default as
  the initial selection and Doom as the only shipped alternative, alongside custom WAV import.
  Embed the audio in the desktop binary and persist the choice. Windows retains its native sound.
- Add a separate notification sound toggle for every mode and custom WAV import on macOS/Linux.
  Keep a validated local copy with a system-default reset; preserve the prior sound on import errors.
  Windows retains its system sound because native toasts cannot play uploaded files outside the app
  package. Linux playback depends on the desktop notification service.

- Scroll long scratchpad, todo, agent, and feedback indexes within the available space. Show
  newest scratchpads, todos, and agents first by default; feedback stays ordered by latest update.
  Include every todo and active or archived scratchpad in the desktop, removing the 200-item cutoff.

- Clarify the saved Computer notifications setting: turn it off for in-app only alerts, including
  hiding Dock/taskbar badges while retaining Workman's unread notifications. Add an Open system
  settings shortcut when notification permission is blocked.

- Extend native agent notifications to Windows and Linux: open the matching agent on click, clear
  read alerts, and show a numeric Windows taskbar overlay. Linux integration follows desktop capabilities.

- Keep agent completions unread while Workman is unfocused or minimized, and clear the matching
  macOS Notification Center entries after reading them. Add a default-on top-level agent banner filter.

- Keep recent agent prompts in local history, including template instructions, model settings, and
  attachment references. Copy them or reopen them as a draft, including from a stopped agent's footer.
- Dictate new-agent and template instructions with the same local microphone and Whisper pipeline
  used for Recorded Feedback, without needing screen capture. Keep temporary recording directories
  private and accessible so macOS voice input can create its audio file.
- Move the collapsed Model settings section above agent instructions, alongside template options.

### Recorded Feedback

- Append more recordings to existing feedback and review delivery history. Browse archived
  recordings and restore them when work needs to continue.
- Share the recording pipeline with Windows, including capture, annotation, and audio handling,
  while keeping unavailable recording shortcuts from blocking the session. Windows binaries
  still require a separate Windows build and are not included in this release's download set.
- Enlarge feedback delivery actions, join a compact agent picker to Send, and align destination
  buttons in a wrapping row. Keep unavailable agents visible with status icons and simplify
  feedback sidebar entries to a single title line.
- Automatically archive feedback after a confirmed send to an agent or scratchpad, with an opt-out
  in Feedback settings. Failed sends, copies, and newer unsent edits stay active.

## 0.1.14 - 2026-09-03

Workman 0.1.14 introduces Recorded Feedback on macOS: capture narrated, annotated screen feedback
and hand the resulting transcript and screenshots directly to the place where work will continue.

### Recorded Feedback

- Record the display and microphone from a movable always-on-top toolbar, annotate the screen, and
  capture any number of full-display or selected-region snapshots in sequence.
- Pause and resume recording, mute the microphone, switch input devices, and use configurable start
  and stop shortcuts without leaving the feedback session.
- Preserve annotations while selecting a region, visibly confirm each capture, recover cleanly from
  stopped or interrupted recordings, and refocus Workman when recording ends.
- Review timestamped transcripts and embedded captures in a dedicated feedback view, then archive,
  restore, reorder, or revisit past recordings from the project sidebar.

### Agent and scratchpad handoff

- Send feedback text and images directly into an existing agent at their original timeline
  positions, or create an agent that receives the feedback automatically as soon as it is ready.
- Customize the neutral feedback-introduction prompt in global settings so the recording supplies
  context without instructing the receiving agent to take an action.
- Embed feedback images inside scratchpads instead of linking to local files, and open a movable,
  resizable comment composer beside the selected scratchpad text.
- Place pasted image placeholders at the cursor in New Agent prompts and replace them with the real
  images at the same positions when the initial message is delivered.

### Desktop integration

- Reorder sidebar sections, hide Recorded Feedback entirely, and use its context menu or middle
  click to archive recordings consistently with other project items.
- Keep Recorded Feedback macOS-only while unsupported platforms show no unusable recording action.
- Improve macOS screen-recording permission detection and settings navigation, and expand the
  development installer to clean stale app processes and permission state when explicitly asked.

## 0.1.13 - 2026-09-01

Workman 0.1.13 makes agent launches and stopped sessions more reliable, clarifies process controls,
and adds more expressive project and folder organization.

### Agent orchestration

- Preserve long, multiline agent-template instructions and per-launch input as one initial turn by
  following each terminal's negotiated bracketed-paste protocol, avoiding truncated prompts without
  adding another agent response cycle.
- Add structured model and effort defaults to agent templates, plus per-launch overrides in New
  Agent advanced settings. Claude models such as Fable and Codex models can be paired with a
  supported effort level while other launch arguments remain available.
- Teach waiting parent agents to end their turn after arming an idle timer instead of polling, and
  repeat the no-poll wake-up contract in MCP server guidance, tool schemas, help, launch context,
  and timer results while preserving delivery overrides and requiring a fresh status check on wake.

### Terminals and process controls

- Keep ANSI styling, cursor state, and terminal layout visible after an agent, terminal, or command
  stops, exits, or crashes, with an inline Resume agent, Start terminal, or Run again action instead
  of replacing the session with flattened output.
- Make terminal attachment more resilient to daemon latency with fast-path geometry and bounded
  retries, so opening a busy agent is less likely to require another sidebar click.
- Streamline context menus with icons, action colors, and grouped secondary commands; distinguish a
  graceful Stop from Force stop and from removing the saved sidebar entry.

### Projects, folders, and settings

- Add optional name colors for projects and folders, plus a searchable icon library for replacing
  the standard folder glyph while retaining the familiar default.
- Fold worktree creation, adoption, and removal progress into the matching project row and remove
  duplicate or lingering operation rows after completion.
- Keep modal actions padded and reachable in compact windows, describe settings as application-wide,
  and use platform-neutral “saved locally” status copy.

### Documentation

- Rework the README around practical workflows with a current workspace screenshot, and move deeper
  architecture and contributor detail into a dedicated technical guide.

## 0.1.12 - 2026-08-27

Workman 0.1.12 streamlines agent and project navigation, opens release downloads for public use,
and strengthens the repository for outside contributors.

### Agent and worktree creation

- Replace the add-agent dropdowns with a compact template and model/tool roster, keep template
  details and overrides available on demand, and make additional instructions clearly optional.
- Preserve valid template overrides across reselection, fail closed for stale choices, and allow
  agents to launch without extra instructions.
- Reuse each repository's remembered environment-file preference when creating, forking, or
  adopting worktrees.

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
