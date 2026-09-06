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
The same page offers three **Notification mode** choices: **All agents**, **Only top-level agents**
(the default), or **Only when all agents are ready**. These filter completion/input banners; crash,
timer, and task alerts still notify in every mode. Existing saved switches migrate to the selector.
Viewing an agent in the focused window for three continuous seconds clears its unread state and
matching OS alerts. Its blue dot stays visible while Workman is inactive, even on the selected agent;
switching away or selecting a different agent restarts the viewing delay. macOS application activation
and WebView focus are checked alongside window focus. Explicit Mark read actions clear immediately.
Unread state remains available in Workman when OS notifications are disabled.

Choose **Only when all agents are ready** for a banner when every agent in a project, including children, has stopped
working or starting. Idle agents, input prompts, timer waits, and stopped processes count as ready;
commands and terminals do not delay the alert. The project must have been busy in this daemon
session and remain ready for two seconds, which avoids startup alerts and brief handoff gaps.
Readiness records appear in Workman's notification center and open the project when clicked.
Resuming work retires the previous ready alert.

Project-ready mode suppresses individual completion/input banners instead of replaying them in a
burst. Individual activity remains available in Workman. Switching back to either agent mode restores
individual banners.

**Notification sound** defaults on and can be switched off separately in any mode. It controls
all computer alerts. Existing Project-ready sound preferences migrate to this switch. The default is
the native system sound on macOS/Windows and the desktop theme sound on Linux. Volume, notification
sound permissions, and Focus/Do Not Disturb still determine whether it is audible. Automatic alerts
play audio through the notification service; in-app-only mode produces no automatic computer banner or sound.
Computer notifications retain live status updates while minimized, independently of Keep Awake.

The sound dropdown starts with **System default** and **Doom**, the only bundled alternative.
Doom uses the supplied `dsitemup.wav` pickup sound, converted without changing its timing to 16-bit
PCM and embedded as `dsitempickup.wav` in the desktop binary. Selecting it needs no download or
file dialog, and the choice survives restart. It uses the same native delivery path and respects
the sound toggle and in-app-only setting.

On macOS and Linux, **Choose sound…** imports a WAV file (16-bit PCM, mono or stereo, 8–48 kHz,
less than 30 seconds, up to 6 MiB). Workman validates the actual audio and keeps its own local copy;
cancelled or failed imports retain the previous selection. Selecting **System default** removes the
saved copy. A selected upload appears as **Custom: filename** in the dropdown. Missing files fall
back to the system sound without blocking the banner. macOS stores its uniquely named copy in `~/Library/Sounds` for Notification Center; Linux uses Workman's app data
folder and the standard `sound-file` hint. Linux desktops may ignore sound hints. Windows continues
to use its system sound and disables Doom/importing: native toast audio requires Windows package
resources, and the current Win32 build does not have that package identity. Embedding bytes in the
executable does not supply a Windows package resource URI.

Click the speaker beside the sound dropdown for an immediate preview. Doom and custom WAVs play
from the saved local copy; System default previews the macOS alert sound, Windows notification
sound, or Linux theme event. Linux previews use `canberra-gtk-play`, with `paplay`/`aplay` fallbacks
for saved WAVs. Playback errors appear beside the sound controls. Preview is a deliberate action
and works even when automatic notifications or sounds are turned off; it does not send a banner.

**Volume** adjusts Doom and custom WAVs from 0–100%, relative to system volume. The saved level
applies to previews and automatic notifications using the same attenuated PCM copy; original audio
is never modified. Damaged rendered copies are rebuilt; the cache keeps only the current and
previous volume copies. Previews block sound changes but do not delay automatic alerts. System default volume stays under OS control, including on Windows.

Use **Send test in 5s** to try the saved sound and banner settings, with time to switch to another app.
Permission is resolved before the countdown, which runs in native code even if the WebView is
suspended. The result is retained natively and refreshed when you return. Both stable and dev
window configurations disable background throttling so ordinary agent alerts also keep running.
The test uses the native notification service and the current sound toggle. Leaving notification
settings or turning off computer notifications cancels a test before delivery starts. Once the
OS submission begins, it finishes and retains its result.

When permission is blocked (including macOS allowing banners but disabling sounds),
**Open system settings** opens Workman's notification controls on
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
the project-ready alert, and work resuming while OS permission is pending. Verify all three modes,
Doom selection/reload/default reset and custom sound import on macOS/Linux, invalid/missing files, system-sound fallback
on Windows, and sound suppression under OS notification sound settings and Focus/Do Not Disturb.

## Voice input storage

Voice input verifies the Whisper model on a worker before recording, keeping its large checksum
read off the UI thread. Temporary dictation sessions hold an OS file lock through transcription;
preflight removes abandoned marked sessions older than one minute after a crash, preserving live sessions
and unrelated folders.
