# Changelog

All notable changes to Workman are recorded here.

## Unreleased

## 0.1.1 - 2026-08-05

The first Workman release turns the awm preview into a more complete work manager, with a precise
desktop design system, first-class Git worktrees, durable terminal history, and a migration-safe
rename. Existing awm v0.1.0 installs can update directly to this release.

### Highlights

- Renamed the product from awm to Workman: the terminal command is now `wrk`, the daemon is
  `workmand`, and runtime configuration uses `WORKMAN_*` with a `workman` MCP identity.
- Added a non-destructive first-run migration chain that prefers existing awm state and falls back
  to gbuild, while `workman.yml` retains warned read compatibility with `awm.yml` and `gbuild.yml`.
- Added the one-release updater bridge and transitional awm-named assets required for real v0.1.0
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

The first awm preview is a native, Solo-style workspace for running coding agents beside a
development stack. A durable Rust daemon owns terminals and coordination state while the `awm`
CLI and Tauri desktop app remain reconnectable clients.

### Highlights

- Project and PTY process management with server-rendered terminal history, readiness checks,
  lifecycle automation, trust-gated `awm.yml` commands, and live resource status.
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
