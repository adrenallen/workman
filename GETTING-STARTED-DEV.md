# Workman side-by-side development install

Use the development identity when you want the released Workman daily driver and the current
working tree open at the same time. From the repository root, run:

```sh
scripts/dev-install.sh
```

The macOS build scripts select the active toolchain's SDK with
`xcrun --sdk macosx --show-sdk-path`, preserving an explicit `SDKROOT` override. This avoids
linker errors such as `libSystem.tbd: unknown architecture` when Xcode and the separately
installed Command Line Tools have different SDK versions. For direct Cargo or Tauri builds,
select the same SDK in your shell first:

```sh
export SDKROOT="$(xcrun --sdk macosx --show-sdk-path)"
```

Release builds disable stripping for build-time dependencies to work around a
[Rust/macOS 27 linker issue](https://github.com/rust-lang/rust/issues/157750). It can appear as
`error[E0463]: can't find crate for ctor_proc_macro` even when the compiled macro exists.
Rerun the installer after updating this checkout; Cargo rebuilds the affected dependencies
automatically, so a full `cargo clean` is unnecessary.

The script builds the current checkout and installs three names that never replace the release:

- `wrk-dev` — CLI for the development data directory and daemon.
- `workmand-dev` — development daemon, started automatically by `wrk-dev` or the app.
- `~/Applications/Workman Dev.app` — bundle id `com.workman.dev`, with an amber `DEV` Dock badge.

After a successful build, the installer stops the existing Dev app and daemon, replaces the
binaries and app, and registers that exact bundle with macOS. It then removes build output and
opens the installed app automatically. Run it from Terminal.app
or another shell outside Workman Dev: stopping the daemon also stops the agents and terminals it
hosts, and the installer refuses to terminate its own parent daemon.

On macOS, the zero-flag defaults are:

| Identity | CLI / daemon | Data and config | Desktop app |
| --- | --- | --- | --- |
| Stable | `wrk` / `workmand` | `~/Library/Application Support/workman` | `Workman.app` (`com.workman.desktop`) |
| Dev | `wrk-dev` / `workmand-dev` | `~/Library/Application Support/workman-dev` | `Workman Dev.app` (`com.workman.dev`) |

The discovery files and dynamic loopback ports live in their respective data directories. The MCP
setup printed by `wrk-dev mcp-setup` uses the server name `workman-dev` and a separate authorization
environment variable, so it can coexist with the stable `workman` registration.

Start the two identities independently:

```sh
wrk app
wrk-dev app
```

`wrk-dev update` intentionally does not download or install a release. It prints a reminder to
rerun `scripts/dev-install.sh`, which is the only supported way to refresh the current-tree dev
identity. Stable `wrk update` continues to manage only the stable `wrk`/`workmand` installation.

The local installer accepts environment overrides for isolated testing or alternate locations:
`WORKMAN_DEV_BIN_DIR`, `WORKMAN_DEV_INSTALL_DIR`, `WORKMAN_DEV_APP_PATH`, and
`WORKMAN_DEV_BUILD_DIR`.

After a successful install, the installer removes build output and runs the installed
`wrk-dev app` command without prompting, in both interactive and non-interactive runs.
Skip either step with its opt-out flag:

```sh
scripts/dev-install.sh                           # Install, clean build output, and open the app
scripts/dev-install.sh --no-cleanup              # Keep build output and open the app
scripts/dev-install.sh --no-relaunch             # Clean build output and leave the app closed
scripts/dev-install.sh --no-cleanup --no-relaunch # Keep build output and leave the app closed
```

Cleanup removes the Cargo target directory (including other cached build profiles) and
`apps/desktop/dist`. It respects `WORKMAN_DEV_BUILD_DIR` and preserves the installed app, binaries,
and application data. The next build will take longer; use `--no-cleanup` for faster rebuilds.
Failed installs do not run cleanup or launch the app.

`--cleanup` and `--relaunch` explicitly enable the default behavior. `WORKMAN_DEV_RELAUNCH=1`
(the default) or `0` can also control launching; command-line flags take precedence. Set it to
`ask` to request an interactive launch prompt instead (non-interactive runs then leave the app
closed). Cleanup runs before launching, using the installed CLI even after the build cache is
removed. A cleanup failure still allows the launch attempt and leaves the installer with a
nonzero exit status.

Screen Recording and Microphone access are preserved when the rebuilt app has the same stable
code-signing requirement. The installer automatically resets both permissions when that identity
changes or only ad hoc signing is available, so macOS can grant them to the newly installed app.
Pass `--reset-permissions` to force that repair when System Settings and Workman disagree; the next
feedback recording will ask for access again.
