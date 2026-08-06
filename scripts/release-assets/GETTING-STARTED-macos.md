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

1. Move this whole extracted folder somewhere permanent. The command-line installer creates
   links back to its `bin/` directory.
2. Drag `Workman.app` into `/Applications`, then double-click it.
3. Browser-downloaded ZIPs receive the `com.apple.quarantine` attribute. Because Workman is
   unsigned, Gatekeeper may block the first launch. Remove the attribute in Terminal:

   ```sh
   xattr -dr com.apple.quarantine /Applications/Workman.app
   ```

   Or open **System Settings → Privacy & Security** and choose **Open Anyway** for Workman.
   Installs run through the CLI installer path do not receive browser quarantine.
4. Open `Workman.app` again. Choose a project directory and Workman will start its background
   daemon automatically.

## Add the terminal command

From this folder, run:

```sh
./install.sh
```

The installer links `wrk` and `workmand` into `~/.local/bin`, prints a PATH hint when needed, and
offers to copy `Workman.app` to `/Applications`. Afterward, try `wrk --help`. Again, `workmand` is
an internal background service; do not launch it yourself.

## Updates

Check for the newest stable release with:

```sh
wrk update --check
```

Install an available command-line update with `wrk update`. The command will tell you when the
desktop app also needs to be replaced from the new platform bundle.
