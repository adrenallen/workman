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

Settings → Notifications → Computer notifications switches between in-app only and in-app plus
computer notifications. Turning it off stops new system alerts and hides Dock/taskbar badges without
changing in-app unread items. The choice is saved locally and also controls non-agent system alerts.
The same page controls the default top-level agent filter.
Viewing an agent in the focused window clears its unread state and matching OS alerts. Unread
state remains available in Workman when OS notifications are disabled.

Enable **Project ready** for a banner when every agent in a project, including children, has stopped
working or starting. Idle agents, input prompts, timer waits, and stopped processes count as ready;
commands and terminals do not delay the alert. The project must have been busy in this daemon
session and remain ready for two seconds, which avoids startup alerts and brief handoff gaps.
Readiness records appear in Workman's notification center and open the project when clicked.
Resuming work retires the previous ready alert.

**Wait until the project is ready** enables project-ready banners and suppresses individual agent
completion/input banners. It does not replay those banners when the project finishes. Individual
activity remains in Workman, and crash, timer, and task alerts retain their existing behavior.
Turning Project ready off restores individual banners. Both new banner options default off.

**Project-ready sound** defaults on for enabled project-ready banners and can be switched off
separately. It uses the native system sound on macOS and Windows, and standard sound hints on Linux;
desktop sound support, volume, and Focus/Do Not Disturb determine whether it is audible. In-app-only
mode produces no computer banner or sound. Computer notifications retain live status updates while
the window is minimized, independently of Keep Awake.

When permission is blocked, **Open system settings** opens Workman's notification controls on
macOS or the Notifications page on Windows. Linux shortcuts support GNOME, KDE Plasma, and Xfce;
other desktops show manual directions. Permission refreshes when Workman regains focus.

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
Also check two working agents finishing separately (including a child), one project becoming ready
while another remains busy, sound on/off, a quick handoff restarting the two-second pause, clicking
the project-ready alert, and work resuming while OS permission is pending.
