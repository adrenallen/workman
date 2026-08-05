# Workman desktop

The desktop shell is a Tauri 2 application with a Svelte 5 frontend. It remains a thin client:
the Rust side discovers or starts `workmand`, authenticates to its loopback WebSocket, and forwards
control frames to the webview as Tauri events.

```sh
npm install
npm run build
npm run tauri dev
```

Set `WORKMAN_DAEMON_BIN` to use a specific `workmand`. The desktop prefers a sibling daemon binary
and otherwise starts a headless daemon process from its own executable, so a standalone Tauri
build still has working auto-spawn behavior.
