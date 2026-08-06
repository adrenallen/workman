import assert from "node:assert/strict";
import { spawnSync } from "node:child_process";
import { fileURLToPath } from "node:url";
import test from "node:test";

const installer = fileURLToPath(new URL("../install.sh", import.meta.url));

test("bootstrap installer has valid shell syntax and documents both key forms", () => {
  const syntax = spawnSync("bash", ["-n", installer], { encoding: "utf8" });
  assert.equal(syntax.status, 0, syntax.stderr);

  const help = spawnSync("bash", [installer, "--help"], { encoding: "utf8" });
  assert.equal(help.status, 0, help.stderr);
  assert.match(help.stdout, /--key <download-key>/);
  assert.match(help.stdout, /WORKMAN_KEY=<download-key>/);
});

test("bootstrap installer refuses to fetch without a key", () => {
  const environment = { ...process.env };
  delete environment.WORKMAN_KEY;
  const result = spawnSync("bash", [installer], { encoding: "utf8", env: environment });
  assert.equal(result.status, 2);
  assert.match(result.stderr, /download key is required/i);
});
