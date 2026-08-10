<p align="center">
  <img src="https://raw.githubusercontent.com/adrenallen/workman/main/assets/branding/workman-logo-wide.png" alt="Workman" width="720">
</p>

# Getting started with Workman on Linux

This download contains all three parts of Workman.

- **`Workman.AppImage` is the desktop app.** Run this for the visual workspace.
- **`bin/wrk` is the terminal command.** Use it to open or control Workman from a shell.
- **`bin/workmand` is the background daemon.** The AppImage and `wrk` command start it
  automatically. You never run `workmand` by hand.

Linux desktop support is experimental. If you prefer a system package, download the separate
`.deb` for your architecture instead of this portable archive.

## First run

1. Extract the archive anywhere convenient. The command-line installer copies its binaries to a
   durable location in your home directory.
2. Start the desktop app:

   ```sh
   chmod +x ./Workman.AppImage
   ./Workman.AppImage
   ```

3. Choose a project directory. Workman starts its background daemon automatically.

## Add the terminal command

From this folder, run:

```sh
./install.sh
```

The installer copies `wrk` and `workmand` to
`~/.local/share/workman/dist/<version>/bin/`, links them into `~/.local/bin`, and prints a PATH
hint when needed. You can delete the extracted folder afterward. Try `wrk --help`; `workmand` is
an internal background service, so do not launch it yourself.

## Updates

Check for the newest stable release with:

```sh
wrk update --check
```

Install an available command-line update with `wrk update`. Replace the AppImage from the new
platform bundle when the command reports that a desktop update is available.
