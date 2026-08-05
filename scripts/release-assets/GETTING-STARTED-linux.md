# Getting started with awm on Linux

This download contains all three parts of awm.

- **`awm.AppImage` is the desktop app.** Run this for the visual workspace.
- **`bin/awm` is the terminal command.** Use it to open or control awm from a shell.
- **`bin/awmd` is the background daemon.** The AppImage and `awm` command start it
  automatically. You never run `awmd` by hand.

Linux desktop support is experimental. If you prefer a system package, download the separate
`.deb` for your architecture instead of this portable archive.

## First run

1. Extract the archive somewhere permanent. The command-line installer creates links back to
   its `bin/` directory.
2. Start the desktop app:

   ```sh
   chmod +x ./awm.AppImage
   ./awm.AppImage
   ```

3. Choose a project directory. awm starts its background daemon automatically.

## Add the terminal command

From this folder, run:

```sh
./install.sh
```

The installer links `awm` and `awmd` into `~/.local/bin` and prints a PATH hint when needed.
Try `awm --help` afterward. `awmd` is an internal background service; do not launch it yourself.

## Updates

Check for the newest stable release with:

```sh
awm update --check
```

Install an available command-line update with `awm update`. Replace the AppImage from the new
platform bundle when the command reports that a desktop update is available.
