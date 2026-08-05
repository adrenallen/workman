# Changelog

All notable changes to awm are recorded here.

## Unreleased

- Added stable and latest update channels, prerelease-first publishing, and an explicit release
  promotion command.
- Made routine CI manual-only so builds run only when a maintainer requests them or pushes a
  release tag.

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
