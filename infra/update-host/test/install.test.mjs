import assert from "node:assert/strict";
import { spawnSync } from "node:child_process";
import {
  chmodSync,
  mkdirSync,
  mkdtempSync,
  readFileSync,
  readdirSync,
  realpathSync,
  rmSync,
  writeFileSync,
} from "node:fs";
import { createHash } from "node:crypto";
import { arch, platform, tmpdir } from "node:os";
import { join } from "node:path";
import { fileURLToPath } from "node:url";
import test from "node:test";

const installer = fileURLToPath(new URL("../install.sh", import.meta.url));
const bundledInstaller = fileURLToPath(
  new URL("../../../scripts/release-assets/install.sh", import.meta.url),
);
const requiredShells = ["sh", "bash"];
const optionalShells = ["dash"];

function availableShells() {
  return [...requiredShells, ...optionalShells].filter((shell) => {
    const probe = spawnSync(shell, ["-c", "exit 0"], { encoding: "utf8" });
    if (probe.error?.code === "ENOENT") {
      assert.ok(optionalShells.includes(shell), `required shell not found: ${shell}`);
      return false;
    }
    assert.equal(probe.status, 0, probe.stderr);
    return true;
  });
}

function executable(path, source) {
  writeFileSync(path, source, { mode: 0o755 });
  chmodSync(path, 0o755);
}

function createMacApp(bundle, identifier, marker) {
  const contents = join(bundle, "Contents");
  const executableDir = join(contents, "MacOS");
  mkdirSync(executableDir, { recursive: true });
  executable(join(executableDir, "workman-desktop"), `#!/bin/sh\nprintf '${marker}\\n'\n`);
  writeFileSync(join(contents, "Info.plist"), `<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0"><dict>
<key>CFBundleIdentifier</key><string>${identifier}</string>
<key>CFBundleExecutable</key><string>workman-desktop</string>
</dict></plist>
`);
}

function packageTarget() {
  if (platform() === "darwin" && arch() === "arm64") return ["macos-arm64", "zip"];
  if (platform() === "linux" && arch() === "x64") return ["linux-x86_64", "tar"];
  if (platform() === "linux" && arch() === "arm64") return ["linux-arm64", "tar"];
  throw new Error(`unsupported installer test platform: ${platform()} ${arch()}`);
}

function createInstallFixture(shell) {
  const root = mkdtempSync(join(tmpdir(), `workman-installer-${shell}-`));
  const bundle = join(root, "bundle");
  const bundleBin = join(bundle, "bin");
  const oldBin = join(root, "old-bin");
  const lateBin = join(root, "late-bin");
  const tools = join(root, "tools");
  const home = join(root, "home");
  const installDir = join(root, "install");
  const distBinDir = join(home, ".local", "share", "workman", "dist", "9.9.9", "bin");
  mkdirSync(bundleBin, { recursive: true });
  mkdirSync(oldBin, { recursive: true });
  mkdirSync(lateBin, { recursive: true });
  mkdirSync(tools, { recursive: true });
  mkdirSync(home, { recursive: true });
  mkdirSync(installDir, { recursive: true });

  executable(join(bundleBin, "wrk"), "#!/bin/sh\nprintf 'workman 9.9.9\\n'\n");
  executable(join(bundleBin, "workmand"), "#!/bin/sh\nexit 0\n");
  executable(join(bundle, "release-install.sh"), readFileSync(bundledInstaller, "utf8"));
  executable(
    join(bundle, "install.sh"),
    `#!/bin/bash
set -eu
bundle_dir="$(cd "$(dirname "$0")" && pwd)"
bash "$bundle_dir/release-install.sh" </dev/null
if [[ -n "$FIXTURE_LATE_SHADOW" ]]; then
  printf '#!/bin/sh\nprintf "workman 0.1.1\\n"\n' > "$FIXTURE_LATE_SHADOW/wrk"
  chmod +x "$FIXTURE_LATE_SHADOW/wrk"
fi
`,
  );
  for (const program of ["wrk", "awm"]) {
    executable(join(oldBin, program), "#!/bin/sh\nprintf 'workman 0.1.1\\n'\n");
  }
  for (const program of ["workmand", "awmd"]) {
    executable(join(oldBin, program), "#!/bin/sh\nexit 0\n");
  }
  if (platform() === "darwin") {
    createMacApp(join(bundle, "Workman.app"), "com.workman.desktop", "new-app");
    createMacApp(
      join(root, "Applications", "Workman.app"),
      "com.workman.desktop",
      "old-app",
    );
  }

  const [target, kind] = packageTarget();
  const archive = join(root, kind === "zip" ? "release.zip" : "release.tar.gz");
  const packaged = kind === "zip"
    ? spawnSync("zip", ["-qr", archive, "."], { cwd: bundle, encoding: "utf8" })
    : spawnSync("tar", ["-czf", archive, "-C", bundle, "."], { encoding: "utf8" });
  assert.equal(packaged.status, 0, packaged.stderr);
  const checksum = createHash("sha256").update(readFileSync(archive)).digest("hex");
  const manifest = join(root, "releases.json");
  writeFileSync(manifest, JSON.stringify({
    channels: {
      stable: {
        version: "9.9.9",
        assets: [{
          target,
          sha256: checksum,
          url: "https://fixture.invalid/versions/9.9.9/release",
        }],
      },
    },
  }));
  executable(
    join(tools, "curl"),
    `#!/bin/sh
output=
url=
while [ "$#" -gt 0 ]; do
  case "$1" in
    --output) output="$2"; shift 2 ;;
    http://*|https://*) url="$1"; shift ;;
    *) shift ;;
  esac
done
case "$url" in
  */releases.json) cp "$FIXTURE_MANIFEST" "$output" ;;
  *) cp "$FIXTURE_ARCHIVE" "$output" ;;
esac
`,
  );
  const path = [lateBin, oldBin, oldBin, tools, join(home, ".local", "bin"), "/usr/bin", "/bin", "/usr/sbin", "/sbin"].join(":");
  return { root, oldBin, lateBin, home, installDir, distBinDir, manifest, archive, path };
}

function executeInstallFixture(fixture, shell, extraArguments = ["--yes"], extraEnv = {}) {
  const result = spawnSync(shell, [installer, "--key", "fixture-key", ...extraArguments], {
    encoding: "utf8",
    env: {
      ...process.env,
      HOME: fixture.home,
      PATH: fixture.path,
      WORKMAN_INSTALL_DIR: fixture.installDir,
      WORKMAN_INSTALL_TEST_ROOT: fixture.root,
      WORKMAN_UPDATE_BASE_URL: "https://fixture.invalid",
      FIXTURE_MANIFEST: fixture.manifest,
      FIXTURE_ARCHIVE: fixture.archive,
      FIXTURE_LATE_SHADOW: "",
      ...extraEnv,
    },
  });
  return result;
}

function runInstallFixture(shell, extraArguments = ["--yes"]) {
  const fixture = createInstallFixture(shell);
  const result = executeInstallFixture(fixture, shell, extraArguments);
  return { fixture, result };
}

test("bootstrap installer has valid shell syntax and documents key and channel forms", () => {
  for (const shell of availableShells()) {
    const syntax = spawnSync(shell, ["-n", installer], { encoding: "utf8" });
    assert.equal(syntax.status, 0, `${shell}: ${syntax.stderr}`);

    const help = spawnSync(shell, [installer, "--help"], { encoding: "utf8" });
    assert.equal(help.status, 0, `${shell}: ${help.stderr}`);
    assert.match(help.stdout, /--key <download-key>/);
    assert.match(help.stdout, /WORKMAN_KEY=<download-key>/);
    assert.match(help.stdout, /--channel <channel>/);
    assert.match(help.stdout, /WORKMAN_CHANNEL=latest/);
    assert.match(help.stdout, /--yes/);
  }

  const source = readFileSync(installer, "utf8");
  assert.doesNotMatch(source, /\[\[/);
  assert.doesNotMatch(source, /<\s*<\(/);
  assert.doesNotMatch(source, /\$'[^']*'/);
});

test("bootstrap follows the bundled installer's durable dist layout while replacing launchers", () => {
  for (const shell of availableShells()) {
    const { fixture, result } = runInstallFixture(shell);
    try {
      assert.equal(result.status, 0, `${shell}: ${result.stderr}\n${result.stdout}`);
      assert.doesNotMatch(result.stdout, /\bawm(?:d)?\b/i);
      assert.doesNotMatch(result.stderr, /\bawm(?:d)?\b/i);
      assert.match(result.stdout, /Selected Workman 9\.9\.9 from the stable channel/);
      assert.match(result.stdout, /Verified fresh PATH resolution: .* reports workman 9\.9\.9/);
      assert.match(result.stdout, /Note: fresh PATH resolution uses .*old-bin\/wrk/);
      assert.match(result.stdout, /run: hash -r/);
      assert.equal(
        realpathSync(join(fixture.home, ".local", "bin", "wrk")),
        realpathSync(join(fixture.distBinDir, "wrk")),
      );
      const wrkLines = result.stdout
        .split("\n")
        .filter((line) => line.trimStart().startsWith("wrk") && line.includes(`${fixture.oldBin}/wrk`));
      assert.equal(wrkLines.length, 1, `duplicate PATH entry was not deduplicated:\n${result.stdout}`);
      for (const [program, target] of [
        ["wrk", "wrk"],
        ["awm", "wrk"],
        ["workmand", "workmand"],
        ["awmd", "workmand"],
      ]) {
        assert.equal(
          realpathSync(join(fixture.oldBin, program)),
          realpathSync(join(fixture.distBinDir, target)),
        );
        assert.ok(
          readdirSync(fixture.oldBin).some((name) => name.startsWith(`${program}.workman-backup-`)),
          `missing backup for ${program}`,
        );
      }
      if (platform() === "darwin") {
        const installedApp = join(fixture.root, "Applications", "Workman.app");
        assert.match(result.stdout, /available in Launchpad and Spotlight/);
        assert.equal(
          spawnSync(join(installedApp, "Contents", "MacOS", "workman-desktop"), [], {
            encoding: "utf8",
          }).stdout.trim(),
          "new-app",
        );
      }
    } finally {
      rmSync(fixture.root, { recursive: true, force: true });
    }
  }
});

test("non-tty bootstrap proceeds with replacement like --yes", () => {
  const { fixture, result } = runInstallFixture("sh", []);
  try {
    assert.equal(result.status, 0, `${result.stderr}\n${result.stdout}`);
    assert.match(result.stdout, /No interactive terminal; proceeding with replacement/);
    assert.equal(
      realpathSync(join(fixture.oldBin, "wrk")),
      realpathSync(join(fixture.distBinDir, "wrk")),
    );
  } finally {
    rmSync(fixture.root, { recursive: true, force: true });
  }
});

test("macOS bootstrap refuses to replace an app with a foreign bundle id", { skip: platform() !== "darwin" }, () => {
  const fixture = createInstallFixture("sh");
  const foreignApp = join(fixture.root, "Applications", "Workman.app");
  rmSync(foreignApp, { recursive: true });
  createMacApp(foreignApp, "com.example.someone-elses-app", "foreign-app");
  const result = executeInstallFixture(fixture, "sh");
  try {
    assert.notEqual(result.status, 0, result.stdout);
    assert.match(result.stderr, /refusing to replace .*bundle id.*not 'com\.workman\.desktop'/);
    assert.equal(
      spawnSync(join(foreignApp, "Contents", "MacOS", "workman-desktop"), [], {
        encoding: "utf8",
      }).stdout.trim(),
      "foreign-app",
    );
  } finally {
    rmSync(fixture.root, { recursive: true, force: true });
  }
});

test("bootstrap fails loudly when a new shadowing wrk appears after inventory", () => {
  const fixture = createInstallFixture("sh");
  const result = executeInstallFixture(fixture, "sh", ["--yes"], {
    FIXTURE_LATE_SHADOW: fixture.lateBin,
  });
  try {
    assert.equal(result.status, 1, result.stdout);
    assert.match(
      result.stderr,
      new RegExp(`fresh PATH walk still selects ${fixture.lateBin}/wrk.*not the just-installed`),
    );
    assert.match(result.stderr, new RegExp(`offending launcher ${fixture.lateBin}/wrk`));
    assert.match(result.stdout, /run: hash -r/);
  } finally {
    rmSync(fixture.root, { recursive: true, force: true });
  }
});

test("bootstrap installer rejects unknown release channels before fetching", () => {
  for (const shell of availableShells()) {
    const result = spawnSync(
      shell,
      [installer, "--key", "test-key", "--channel", "nightly"],
      { encoding: "utf8" },
    );
    assert.equal(result.status, 2, `${shell}: ${result.stderr}`);
    assert.match(result.stderr, /expected stable or latest/i);
  }
});

test("bootstrap installer refuses to fetch without a key", () => {
  const environment = { ...process.env };
  delete environment.WORKMAN_KEY;
  for (const shell of availableShells()) {
    const result = spawnSync(shell, [installer], { encoding: "utf8", env: environment });
    assert.equal(result.status, 2, `${shell}: ${result.stderr}`);
    assert.match(result.stderr, /download key is required/i);
  }
});
