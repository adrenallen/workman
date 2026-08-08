import assert from "node:assert/strict";
import { createHash } from "node:crypto";
import { mkdtemp, rm, writeFile } from "node:fs/promises";
import { tmpdir } from "node:os";
import { join } from "node:path";
import test from "node:test";
import { generateManifest } from "../scripts/generate-manifest.mjs";

function sha256(content) {
  return createHash("sha256").update(content).digest("hex");
}

test("generates sorted, verified release assets and the checksum asset", async () => {
  const directory = await mkdtemp(join(tmpdir(), "workman-manifest-test-"));
  try {
    const linux = Buffer.from("linux artifact");
    const macos = Buffer.from("macos artifact");
    await writeFile(join(directory, "workman-linux-x86_64.tar.gz"), linux);
    await writeFile(join(directory, "workman-macos-arm64.zip"), macos);
    const sums = [
      `${sha256(macos)}  workman-macos-arm64.zip`,
      `${sha256(linux)}  workman-linux-x86_64.tar.gz`,
      "",
    ].join("\n");
    await writeFile(join(directory, "SHA256SUMS"), sums);

    const manifest = await generateManifest({
      version: "v1.2.3",
      artifactsDir: directory,
      publishedAt: "2026-08-06T12:34:56Z",
      notesUrl: "https://github.com/adrenallen/workman/releases/tag/v1.2.3",
    });

    assert.equal(manifest.version, "1.2.3");
    assert.equal(manifest.published_at, "2026-08-06T12:34:56.000Z");
    assert.deepEqual(manifest.assets.map(({ name }) => name), [
      "workman-linux-x86_64.tar.gz",
      "workman-macos-arm64.zip",
      "SHA256SUMS",
    ]);
    assert.equal(manifest.assets[0].target, "linux-x86_64");
    assert.equal(manifest.assets[2].sha256, sha256(sums));
    assert.equal(
      manifest.assets[1].url,
      "https://workman.userdefined.io/versions/1.2.3/workman-macos-arm64.zip",
    );
  } finally {
    await rm(directory, { recursive: true, force: true });
  }
});

test("rejects an artifact whose content does not match SHA256SUMS", async () => {
  const directory = await mkdtemp(join(tmpdir(), "workman-manifest-test-"));
  try {
    await writeFile(join(directory, "workman-macos-arm64.zip"), "changed");
    await writeFile(
      join(directory, "SHA256SUMS"),
      `${sha256("original")}  workman-macos-arm64.zip\n`,
    );
    await assert.rejects(
      generateManifest({
        version: "1.2.3",
        artifactsDir: directory,
        publishedAt: "2026-08-06T12:34:56Z",
        notesUrl: "https://example.com/v1.2.3",
      }),
      /SHA256 mismatch/,
    );
  } finally {
    await rm(directory, { recursive: true, force: true });
  }
});

test("rejects obsolete pre-Workman release aliases", async () => {
  const directory = await mkdtemp(join(tmpdir(), "workman-manifest-test-"));
  try {
    const name = "awm-linux-x86_64.tar.gz";
    const content = Buffer.from("obsolete alias");
    await writeFile(join(directory, name), content);
    await writeFile(join(directory, "SHA256SUMS"), `${sha256(content)}  ${name}\n`);

    await assert.rejects(
      generateManifest({
        version: "1.2.3",
        artifactsDir: directory,
        publishedAt: "2026-08-06T12:34:56Z",
        notesUrl: "https://example.com/v1.2.3",
      }),
      /obsolete pre-Workman release artifact is not publishable/,
    );
  } finally {
    await rm(directory, { recursive: true, force: true });
  }
});
