#!/usr/bin/env node

import { createHash } from "node:crypto";
import { readFile, stat } from "node:fs/promises";
import { basename, join } from "node:path";
import { fileURLToPath } from "node:url";

const SHA256 = /^[a-f0-9]{64}$/;
const VERSION = /^v?(\d+\.\d+\.\d+)$/;

function parseArguments(values) {
  const parsed = {};
  for (let index = 0; index < values.length; index += 1) {
    const option = values[index];
    if (!option.startsWith("--") || index + 1 >= values.length) {
      throw new Error(`expected --name value, got ${option}`);
    }
    parsed[option.slice(2).replaceAll("-", "_")] = values[index + 1];
    index += 1;
  }
  return parsed;
}

function releaseTarget(name) {
  const targets = new Map([
    ["workman-macos-arm64.zip", "macos-arm64"],
    ["workman-linux-x86_64.tar.gz", "linux-x86_64"],
    ["workman-linux-arm64.tar.gz", "linux-arm64"],
    ["workman-linux-x86_64.AppImage", "linux-x86_64-appimage"],
    ["workman-linux-arm64.AppImage", "linux-arm64-appimage"],
    ["workman-linux-x86_64.deb", "linux-x86_64-deb"],
    ["workman-linux-arm64.deb", "linux-arm64-deb"],
    ["awm-macos-arm64.tar.gz", "legacy-macos-arm64"],
    ["awm-desktop-macos-arm64.zip", "legacy-desktop-macos-arm64"],
    ["awm-linux-x86_64.tar.gz", "legacy-linux-x86_64"],
    ["awm-linux-arm64.tar.gz", "legacy-linux-arm64"],
    ["awm-desktop-linux-x86_64.AppImage", "legacy-desktop-linux-x86_64"],
    ["awm-desktop-linux-arm64.AppImage", "legacy-desktop-linux-arm64"],
    ["SHA256SUMS", "checksums"],
  ]);
  return targets.get(name) ?? `artifact-${name}`;
}

function parseChecksums(source) {
  const checksums = new Map();
  for (const line of source.split("\n")) {
    if (line.trim() === "") continue;
    const match = /^([a-f0-9]{64})\s+\*?(.+)$/.exec(line);
    if (match === null) throw new Error(`invalid SHA256SUMS line: ${line}`);
    const [, sha256, name] = match;
    if (basename(name) !== name || checksums.has(name)) {
      throw new Error(`unsafe or duplicate artifact name: ${name}`);
    }
    checksums.set(name, sha256);
  }
  if (checksums.size === 0) throw new Error("SHA256SUMS contains no artifacts");
  return checksums;
}

async function fileSha256(path) {
  const content = await readFile(path);
  return createHash("sha256").update(content).digest("hex");
}

export async function generateManifest({
  version: rawVersion,
  artifactsDir,
  publishedAt,
  notesUrl,
  baseUrl = "https://workman.userdefined.io",
}) {
  const versionMatch = VERSION.exec(rawVersion ?? "");
  if (versionMatch === null) throw new Error("version must be X.Y.Z or vX.Y.Z");
  const version = versionMatch[1];
  if (Number.isNaN(Date.parse(publishedAt))) throw new Error("published-at must be an ISO date");
  const notes = new URL(notesUrl);
  const base = new URL(baseUrl);
  if (notes.protocol !== "https:" || base.protocol !== "https:") {
    throw new Error("notes-url and base-url must use https");
  }

  const checksumPath = join(artifactsDir, "SHA256SUMS");
  const checksums = parseChecksums(await readFile(checksumPath, "utf8"));
  const assets = [];
  for (const [name, expectedSha256] of [...checksums].sort(([left], [right]) => left.localeCompare(right))) {
    const path = join(artifactsDir, name);
    const [metadata, actualSha256] = await Promise.all([stat(path), fileSha256(path)]);
    if (!metadata.isFile()) throw new Error(`${name} is not a regular file`);
    if (actualSha256 !== expectedSha256) {
      throw new Error(`${name} SHA256 mismatch: expected ${expectedSha256}, got ${actualSha256}`);
    }
    assets.push({
      name,
      target: releaseTarget(name),
      sha256: expectedSha256,
      size: metadata.size,
      url: `${base.origin}/versions/${version}/${encodeURIComponent(name)}`,
    });
  }

  const checksumMetadata = await stat(checksumPath);
  assets.push({
    name: "SHA256SUMS",
    target: releaseTarget("SHA256SUMS"),
    sha256: await fileSha256(checksumPath),
    size: checksumMetadata.size,
    url: `${base.origin}/versions/${version}/SHA256SUMS`,
  });

  return {
    version,
    published_at: new Date(publishedAt).toISOString(),
    notes_url: notes.href,
    assets,
  };
}

async function main() {
  const args = parseArguments(process.argv.slice(2));
  const manifest = await generateManifest({
    version: args.version,
    artifactsDir: args.artifacts_dir,
    publishedAt: args.published_at,
    notesUrl: args.notes_url,
    baseUrl: args.base_url,
  });
  process.stdout.write(`${JSON.stringify(manifest, null, 2)}\n`);
}

if (process.argv[1] === fileURLToPath(import.meta.url)) {
  main().catch((cause) => {
    process.stderr.write(`${cause instanceof Error ? cause.message : String(cause)}\n`);
    process.exitCode = 1;
  });
}
