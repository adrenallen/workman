import assert from "node:assert/strict";
import { spawnSync } from "node:child_process";
import { readFileSync } from "node:fs";
import { fileURLToPath } from "node:url";
import test from "node:test";

const installer = fileURLToPath(new URL("../install.sh", import.meta.url));
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
  }

  const source = readFileSync(installer, "utf8");
  assert.doesNotMatch(source, /\[\[/);
  assert.doesNotMatch(source, /<\s*<\(/);
  assert.doesNotMatch(source, /\$'[^']*'/);
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
