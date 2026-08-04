# gbuild — Plan

A high-performance reproduction of Solo (soloterm.com): a native terminal workspace for running
AI coding agents alongside a dev stack, with an MCP server that lets the agents see and control
the workspace itself — processes, todos, scratchpads, timers, locks, and each other.

Working name: **gbuild** (daemon `gbuildd`, CLI `gbuild`). Rename freely later.

## Why

Solo's creator has moved on (hired by Laravel; Solo is no longer his focus), but the product
ideas are excellent. We're rebuilding the parts that matter, for personal daily use first:

- **Projects** as the unit of isolation — processes, todos, scratchpads all scoped to a project.
- **Three process kinds**: `command` (trusted, repo-defined dev-stack processes), `terminal`
  (interactive shells), `agent` (Solo-managed agent sessions like Claude Code / Codex).
- **The MCP server is the product.** Agents get tools to inspect and drive the workspace:
  read any process's rendered output, start/stop/restart commands, spawn subagents, send them
  prompts, claim todos, edit shared scratchpads, take locks, and set timers that wake them
  when other agents go idle. This is what turns a process manager into an orchestration surface.
- **Work graphs**: todos with blockers, tags, comments, and edit locks so parallel agents
  claim work without colliding, and unblocking can trigger the next agent.
- **Scratchpads**: revision-guarded markdown buffers agents build plans in, visible in the UI.
- **Repo-committed config** (`gbuild.yml`): command processes with auto_start / auto_restart /
  restart-on-file-change, gated behind an explicit trust review.

## Decisions made

| Decision | Choice |
|---|---|
| App shell | Tauri 2 (system webview — not Electron), Rust everywhere that matters |
| Architecture | Headless daemon owns everything; UI and CLI are thin clients |
| v1 scope | MCP + process tools, todos/scratchpads/locks, agent spawning + timers, `gbuild.yml` |
| Intent | Personal tool first; open-source/product decisions deferred |
| Platform | macOS + Linux supported; Windows via WSL2 (= the Linux build); native Windows deferred |

## Architecture

```
┌─────────────────────────────────────────────────────────────┐
│ gbuildd (daemon, Rust)                                      │
│                                                             │
│  PTY host ──► raw byte ring ──► vt emulation ──► grid state │
│  (portable-pty)                 (alacritty_terminal)        │
│                                                             │
│  SQLite (rusqlite, WAL): projects, processes, todos,        │
│    comments, blockers, scratchpads(+revisions), locks,      │
│    timers, agent tools, actors                              │
│                                                             │
│  Local HTTP server (loopback-only, bearer-token auth):      │
│    /mcp — streamable HTTP MCP endpoint for agents (rmcp)    │
│    /ws — control channel for UI + CLI (JSON/binary)         │
│  Watchers: gbuild.yml sync, restart_when_changed (notify),  │
│    port detection, idle detection, timer scheduler          │
└────────────┬───────────────────────────┬────────────────────┘
             │ /ws (localhost)           │ /mcp (localhost)
      ┌──────┴───────┐            ┌──────┴──────────────┐
      │ Tauri 2 app  │            │ agents (claude,     │
      │ (webview UI) │            │ codex, gemini, ...) │
      └──────────────┘            └─────────────────────┘
      ┌──────────────┐
      │ gbuild CLI   │
      └──────────────┘
```

**Why a daemon:** terminals, agents, and dev servers survive UI restarts; the MCP endpoint
stays up with the window closed; the CLI gets the same API as the UI; headless/remote use
stays possible. The UI process is disposable.

**Terminal pipeline:** the daemon is the single source of truth for terminal state. Every PTY's
raw bytes go to (a) a bounded raw ring buffer (powers `get_process_raw_output` / raw search),
and (b) an `alacritty_terminal` emulator instance (powers rendered output, rendered search, and
the MCP's `get_process_output`). The UI *also* receives the raw byte stream over the control
socket (bridged through the Tauri Rust side) and feeds it to **xterm.js + WebGL addon** for
display. Yes, that's emulation in two places — it's the pragmatic v1: xterm.js/WebGL is a
proven fast renderer with scrollback/search/links for free, and the daemon needs server-side
emulation regardless because agents read rendered output when no UI is attached. If webview
rendering ever becomes the bottleneck, swap the front-end renderer; the daemon contract doesn't
change. Only the *selected* process's stream is attached to the UI; background processes render
nothing (their state lives in the daemon).

**Crate choices:**

- `portable-pty` (WezTerm's) — PTY spawn/resize; Unix-only in practice for us, but keeps
  ConPTY available if native Windows ever matters.
- `alacritty_terminal` — battle-tested vt emulation (also what Zed embeds).
- `rmcp` — official Rust MCP SDK; streamable-HTTP transport on `127.0.0.1:<port>`.
  Register with `claude mcp add --transport http`; no stdio shim needed for Claude Code,
  add one later if some agent requires stdio.
- `rusqlite` (bundled SQLite, WAL mode) — no ORM, plain SQL + migrations.
- `notify` — file watching for `gbuild.yml` sync and `restart_when_changed` globs.
- `sysinfo` + `listeners`/libproc — CPU/mem stats and listening-port detection per pid tree.
- Frontend: **Svelte 5** + xterm.js (WebGL). Small, fast, close to Vue if that's more familiar.

## Data model (SQLite)

- `projects` — id, path (canonical), name, display_name, icon, selected state.
- `processes` — id, project_id, kind (`command`|`terminal`|`agent`), name, command, working_dir,
  env (json), auto_start, auto_restart, restart_when_changed (json globs), source
  (`yml`|`local`), trust_hash, status, pid, exit info, agent_tool_id.
- `agent_tools` — id, name, command, tool_type, enabled (the claude/codex/gemini registry).
- `todos` — id, project_id, title, body, status, priority, completed, tags (junction),
  lock_actor + lock_expiry (lease).
- `todo_blockers` — todo_id, blocked_by_todo_id.
- `todo_comments` — id, todo_id, actor, body, timestamps.
- `scratchpads` — id, project_id, name, content, revision, tags, archived.
  Every write bumps `revision`; writes carrying `expected_revision` fail on mismatch (optimistic
  concurrency — this is how parallel agents don't clobber each other's plans).
- `locks` — project_id, key, owner_actor, acquired_at, ttl (lease; acquisition non-blocking).
- `timers` — id, owner_actor, delivery_process_id, body, kind (`delay`|`idle_any`|`idle_all`),
  watch list (json), interval/loop, max_wait deadline, paused, fired state.
- `actors` — MCP session identities (see below).

Output buffers are **not** in SQLite — in-memory rings per process (raw bytes, bounded ~2–8 MB)
plus the emulator's scrollback. Optional disk spill later.

## MCP design (the important part)

Mirror Solo's tool surface and semantics — they're well designed and already validated:

- **Identity**: every process the daemon spawns gets `GBUILD_PROCESS_ID` plus a unique
  `GBUILD_MCP_TOKEN` in its env. The agent's MCP config sends the token as a header (client
  configs support `${VAR}` expansion), so the daemon maps each HTTP session to the process
  that owns it automatically; `whoami` reports process_id, actor_id, and effective project;
  `identify_session` stays as the manual fallback for externally launched sessions.
- **Scoping**: explicit `project_id` param → session-selected project → identified process's
  project. Every project-scoped tool takes an optional `project_id` override.
- **Tool catalog** (v1, mirroring Solo's categories):
  - *setup*: help (topic-based), whoami, identify_session, mcp_tools_summary,
    mcp_smoke_test (disposable write-read-cleanup self-test)
  - *projects*: list/select/get/status/stats, create (register existing dir), rename,
    delete (confirm-gated; second confirm if processes are running)
  - *processes*: list, status, start/stop/restart, close (self-close confirm), rename,
    select (focus in UI), start/stop/restart_all_commands
  - *output*: get rendered (row-ranged), get raw, search both, clear, send_input
    (text or raw bytes, submit flag, optional wait_ms returning the fresh tail)
  - *spawning*: list_agent_tools, spawn_process(kind=terminal|agent), spawn_agent —
    returns process_id + `agent_instructions` preamble the caller prepends to prompt #1
  - *readiness*: services_list, get_process_ports, wait_for_bound_port
  - *todos*: full CRUD + tags + blockers + comments + lock/unlock + complete + transfer;
    writes return **slim receipts** (`{project_id, todo_id}`) by default, `response_mode=rich`
    opt-in — keeps orchestrator context windows lean
  - *scratchpads*: write/read/append/append_section/edit(section-or-line-range)/find/tail/
    list/rename/tags/archive/clear/delete/transfer, save_to_file/load_from_file —
    all mutations take `expected_revision`
  - *coordination*: lock_acquire/release/status
  - *timers*: timer_set (delay, loop, repeat_every_ms, delivery_process_id),
    timer_fire_when_idle_any/all (watch list + max_wait guard), cancel/pause/resume/list
- **Timer delivery contract**: when a timer fires, its `body` is injected into the delivery
  agent's PTY as if the user typed it — a fresh user turn. This plus idle-watching is the whole
  orchestration trick: "wake me when the worker goes quiet" with zero polling.
- **Attention states**: per-process state machine — `working` (output streaming / busy
  indicators), `needs_input` (quiescent with a pending question: permission prompt, y/n
  confirm, blocked TUI input), `idle` (turn finished, resting at its input prompt), and
  `exited`. The `needs_input` vs `idle` distinction is what keeps orchestration honest — a
  worker stuck on a permission prompt must never read as "done". Detection layers: output
  quiescence window → prompt-row heuristics → per-agent-tool adapters that know each CLI's
  busy/prompt signatures (Claude Code's spinner and its permission dialog look nothing
  alike). Ship quiescence first; grow adapters from observed sessions. Powers idle timers,
  UI status badges, and the agent-state field in get_process_status. That field exposes both
  the derived state and the raw signals (idle_seconds, last_output_at, tool_type, adapter
  flags like thinking/planning) — matching Solo's agent_state shape closely enough that
  orchestration prompts written for Solo translate to gbuild with minimal edits.

## Trust model

`gbuild.yml` commands are code execution from a repo file, so: YAML-backed commands sync into
the DB but cannot run (manually, via MCP, or auto_start) until trusted in the UI. Trust is a
hash over trust-relevant fields (command, working_dir, env, auto_start, auto_restart,
restart_when_changed); any change re-requires review. `working_dir` must stay inside the
project root (reject `..`, absolute-outside, symlink escapes). The daemon binds loopback
only; every request — MCP or UI/CLI — carries a local bearer token, and Origin/Host headers
are validated to block DNS-rebinding from a browser tab.

## `gbuild.yml`

Same shape as solo.yml (keeps mental compatibility):

```yaml
name: My Project
processes:
  Dev server:
    command: npm run dev
    working_dir: ./frontend      # must stay inside project root
    auto_start: true
    auto_restart: false
    restart_when_changed: [src/**/*.ts, package.json]
    env: { NODE_ENV: development }
```

## Platform support

Supported targets: **macOS and Linux**. Windows users run the daemon + CLI inside WSL2 —
which is just the Linux build. A native Windows port is explicitly deferred.

- **Transport**: everything is loopback HTTP/WebSocket. Even without native Windows this
  stays the right call — the MCP endpoint must be HTTP anyway, one server covers UI, CLI,
  and agents, and it's what makes the WSL2 story work: a native Windows UI could later talk
  to a daemon inside WSL2 over localhost port forwarding with zero transport changes.
- **Process semantics are Unix-only**: SIGTERM for graceful stop, process groups for
  tree-kill, `sh -c` invocation. No ConPTY, Job Objects, or `cmd /C` code paths to build
  or test.
- **Daemon lifecycle**: no launchd/systemd dependency — the UI and CLI auto-spawn the daemon
  if it isn't running.
- **Linux UI is first-class**, which promotes the WebKitGTK risk: WebGL there is the least
  reliable of the webviews and xterm.js may degrade to its DOM renderer. Torture-test
  terminal rendering on Linux during M1, not after — that result decides whether the
  custom-renderer escape hatch is ever needed.
- **UI on Windows**: the Tauri app runs under WSLg today; a native Windows window into a
  WSL2 daemon is a later nice-to-have enabled by the HTTP transport.

## Milestones

Each milestone ends in something I use daily. macOS is the daily driver; Linux builds get
checked from M1 on (the terminal-rendering torture test runs on both).

**M0 — Daemon skeleton + CLI (the spine).**
Cargo workspace (`crates/core`, `crates/daemon`, `crates/cli`, `apps/desktop` later).
Daemon boots, opens control socket, SQLite migrations run. Spawn a command process in a PTY,
raw ring buffer, `gbuild run / ps / logs / attach / stop`. Server-side emulation wired in
(rendered output readable via CLI). *Done when: a dev server runs under the daemon, survives
CLI disconnect, and `gbuild attach` gives a live interactive view.*

**M1 — Tauri app: projects, processes, terminal.**
Project sidebar (register existing dirs), process list with status colors (green running /
red crashed), xterm.js WebGL terminal for the selected process, spawn terminals, start/stop/
restart, resize handling, scrollback + search + clickable links. *Done when: I stop using
Terminal.app for dev-stack work in one project.*

**M2 — `gbuild.yml` + trust + lifecycle automation.**
YAML parse/sync/watch, trust review UI, auto_start on project open, auto_restart on crash,
restart_when_changed via glob watching, env injection, saved-command click-to-run panel.
*Done when: cloning a repo with gbuild.yml and trusting it brings up the whole stack.*

**M3 — MCP server: identity + process/output/readiness tools.**
rmcp streamable HTTP on localhost; whoami/identify/help; project scoping rules; the full
process/output/spawning(terminal)/readiness tool set. Register with Claude Code and drive real
sessions. *Done when: an agent can restart my dev server, wait_for_bound_port, read the log
tail, and report the URL — unassisted.*

**M4 — Coordination: todos, scratchpads, locks (+ UI panels).**
Full todo graph (blockers/tags/comments/locks/transfer), revision-guarded scratchpads, lease
locks. UI: todo board with blocker edges visible, rendered-markdown scratchpad viewer that
live-updates as agents write. *Done when: two agents split a task list via todo locks and
build a plan in a scratchpad I watch in the app.*

**M5 — Agents + timers (the orchestration payoff).**
Agent tool registry (claude/codex/gemini/opencode configs), spawn_agent + agent_instructions
preamble, send_input prompting, attention-state detection, timers incl. fire_when_idle_any/all
with PTY-injection delivery, self-close confirmation guard. *Done when: an orchestrator agent
spawns two workers, assigns todos, goes to sleep on an idle timer, wakes when they finish, and
integrates their results.*

**M6 — Polish.**
CPU/mem stats per process tree, themes (Tokyo Night first), open-in-editor, keyboard nav,
bulk command controls, `setup_agent_integration` (write MCP docs into CLAUDE.md), packaging.

## Risks & mitigations

- **Idle detection accuracy** — the orchestration loop depends on it. Mitigate: conservative
  quiescence defaults, per-tool adapters, `max_wait_ms` guard on every idle timer so a stuck
  detector degrades to a timeout, never a hang.
- **Webview terminal perf** — xterm.js WebGL is fast, but test early with `yes`/huge-log
  torture cases in M1. Escape hatch: custom canvas/wgpu renderer later; daemon contract fixed.
- **Dual emulation memory** — bound scrollback per process; make limits configurable.
- **PTY edge cases** (resize storms, alternate screen, bracketed paste) — lean on
  portable-pty + alacritty_terminal rather than hand-rolling; snapshot-test emulation.
- **rmcp API churn** — pin versions; keep MCP tool layer thin over an internal service API
  (the control socket and MCP call the same core services, so tools stay ~glue).

## Testing

- Core: emulation snapshot tests (byte streams → expected grids); todo/blocker/lock/revision
  state-machine tests; yml sync + trust-hash tests.
- MCP: integration tests with an rmcp client exercising every tool against a temp daemon.
- E2E: scripted PTY sessions (spawn shell, send input, assert rendered output).

## Open questions (park for later)

- Real name + icon.
- Scratchpad storage: plain content-with-revision (v1) vs. revision history table.
- Output persistence across daemon restarts (v1: buffers are ephemeral, process table isn't).
- Prompt-template / playbook protocol (Solo exposes MCP prompts) — post-v1.
- Key-value store tools — Solo has them but ships them disabled; skip unless a need appears.
- Multi-window / multiple simultaneous terminal views.
