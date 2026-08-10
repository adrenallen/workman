<p align="center">
  <img src="https://raw.githubusercontent.com/adrenallen/workman/main/assets/branding/workman-logo-wide.png" alt="Workman" width="720">
</p>

# Getting started with Workman on macOS

This download contains all three parts of Workman. Most people start with the app.

- **`Workman.app` is the desktop app.** Double-click this. It gives you the visual workspace.
- **`bin/wrk` is the terminal command.** Use it when you want to open or control Workman from a
  shell.
- **`bin/workmand` is the background daemon.** The app and `wrk` command start it automatically.
  You never run `workmand` by hand.

## First run

1. Keep the extracted folder only as long as you need it. The command-line installer copies its
   binaries to a durable location in your home directory.
2. Drag `Workman.app` into `/Applications`, then double-click it.
3. Workman 0.1.5 and newer are Developer ID signed and notarized, so a browser-downloaded copy
   should pass Gatekeeper and open normally. Releases 0.1.4 and earlier were unsigned; if you are
   intentionally opening one of those older builds, follow the Gatekeeper note bundled with that
   version rather than disabling quarantine for current releases.
4. Choose a project directory and Workman will start its background
   daemon automatically.

## Add the terminal command

From this folder, run:

```sh
./install.sh
```

The installer copies `wrk` and `workmand` to
`~/.local/share/workman/dist/<version>/bin/`, links them into `~/.local/bin`, prints a PATH hint
when needed, and offers to copy `Workman.app` to `/Applications`. You can delete the extracted
folder afterward. Then try `wrk --help`. Again, `workmand` is an internal background service; do
not launch it yourself.

## Updates

Check for the newest stable release with:

```sh
wrk update --check
```

Install an available command-line update with `wrk update`. The command will tell you when the
desktop app also needs to be replaced from the new platform bundle.

## Work on a development build beside this release

From a Workman source checkout, `scripts/dev-install.sh` builds and installs `wrk-dev`,
`workmand-dev`, and a visibly badged `Workman Dev.app`. The dev identity uses its own data/config,
daemon discovery, MCP registration, and `com.workman.dev` bundle id, so this stable installation can
remain open. Run `wrk-dev update` for the reminder to rebuild from the checkout; it never replaces
the stable release. The full workflow is in `GETTING-STARTED-DEV.md` at the repository root.
