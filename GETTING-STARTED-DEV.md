# Workman side-by-side development install

Use the development identity when you want the released Workman daily driver and the current
working tree open at the same time. From the repository root, run:

```sh
scripts/dev-install.sh
```

The script builds the current checkout and installs three names that never replace the release:

- `wrk-dev` — CLI for the development data directory and daemon.
- `workmand-dev` — development daemon, started automatically by `wrk-dev` or the app.
- `~/Applications/Workman Dev.app` — bundle id `com.workman.dev`, with an amber `DEV` Dock badge.

After a successful build, the installer stops the existing Dev app and daemon, replaces the
binaries and app, registers that exact bundle with macOS, and reopens it. Run it from Terminal.app
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
`WORKMAN_DEV_BUILD_DIR`. Pass `--no-relaunch` (or set `WORKMAN_DEV_RELAUNCH=0`) to leave the app
closed after installation.

Screen Recording and Microphone access are preserved when the rebuilt app has the same stable
code-signing requirement. The installer automatically resets both permissions when that identity
changes or only ad hoc signing is available, so macOS can grant them to the newly installed app.
Pass `--reset-permissions` to force that repair when System Settings and Workman disagree; the next
feedback recording will ask for access again.
