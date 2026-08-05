# Getting started with awm on macOS

This download contains all three parts of awm. Most people start with the app.

- **`awm.app` is the desktop app.** Double-click this. It gives you the visual workspace.
- **`bin/awm` is the terminal command.** Use it when you want to open or control awm from a shell.
- **`bin/awmd` is the background daemon.** The app and `awm` command start it automatically. You
  never run `awmd` by hand.

## First run

1. Move this whole extracted folder somewhere permanent. The command-line installer creates
   links back to its `bin/` directory.
2. Double-click `awm.app`. Because this preview is unsigned, macOS may refuse the first launch.
   Control-click the app, choose **Open**, then confirm **Open**.
3. If macOS still says the app is damaged or cannot be verified, open Terminal in this folder
   and run:

   ```sh
   xattr -dr com.apple.quarantine ./awm.app
   ```

4. Double-click `awm.app` again. Choose a project directory and awm will start its background
   daemon automatically.

## Add the terminal command

From this folder, run:

```sh
./install.sh
```

The installer links `awm` and `awmd` into `~/.local/bin`, prints a PATH hint when needed, and
offers to copy `awm.app` to `/Applications`. Afterward, try `awm --help`. Again, `awmd` is an
internal background service; do not launch it yourself.

## Updates

Check for the newest stable release with:

```sh
awm update --check
```

Install an available command-line update with `awm update`. The command will tell you when the
desktop app also needs to be replaced from the new platform bundle.
