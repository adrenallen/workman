# gbuild

gbuild is a native terminal workspace for running AI coding agents alongside a development
stack. A headless Rust daemon will own terminals, process state, persistence, and the local MCP
and control APIs; the CLI and desktop app will be thin clients.

The full project specification, architecture, milestones, and design decisions live in
[PLAN.md](PLAN.md).

## Install

```sh
./install.sh
```

This builds the daemon, CLI, and desktop app in release mode and links them into
`~/.local/bin` without sudo. Re-run it after pulling updates. Then run `gbuild` in any
project directory, `gbuild app` for the desktop workspace, or `gbuild mcp-setup` for Claude Code.

## Workspace

- `crates/gbuild-core` — shared domain and service code
- `crates/gbuildd` — headless daemon binary
- `crates/gbuild-cli` — `gbuild` command-line client
- `apps/desktop` — Tauri 2 desktop app

Run `cargo build --workspace` or `just build` to build the Rust workspace.
