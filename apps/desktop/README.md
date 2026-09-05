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

## Desktop notifications

Settings → Notifications controls background banners and the default top-level agent filter.
Viewing an agent in the focused window clears its unread state and matching OS alerts. Unread
state remains available in Workman when OS notifications are disabled.

- macOS uses Notification Center and the Dock badge.
- Windows uses Workman's Start menu application identity, tagged toasts, and a numeric taskbar
  overlay. Existing shortcut targets, arguments, and icons are preserved when registering the identity.
- Linux uses the session D-Bus notification service. Click actions, retained history, and launcher
  badges depend on the desktop environment. Clearing uses live server-issued IDs during the current
  app session; it does not assume IDs remain valid after restarting the desktop notification service.

The Linux backend's integration test runs against a fake service on a private bus:

```sh
dbus-run-session -- cargo test -p workman-desktop desktop_delivery_and_clear -- --ignored
```

Packaged-app smoke tests should cover a completion while switched away/minimized, clicking its
notification, and reading one agent while a different agent remains unread. Windows and macOS OS
policies (including Do Not Disturb/Focus) can suppress the banner while retaining it in history.
