#!/usr/bin/env node

import { mkdtemp, readFile, rm, writeFile } from "node:fs/promises";
import { tmpdir } from "node:os";
import { basename, join, resolve } from "node:path";
import { spawnSync } from "node:child_process";
import { generateManifest } from "./generate-manifest.mjs";

const BUCKET = "workman-releases";
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

function normalizeVersion(value) {
  const match = VERSION.exec(value ?? "");
  if (match === null) throw new Error("version must be X.Y.Z or vX.Y.Z");
  return match[1];
}

function runWrangler(args) {
  const result = spawnSync("wrangler", args, { stdio: "inherit" });
  if (result.error) throw result.error;
  if (result.status !== 0) throw new Error(`wrangler ${args.slice(0, 3).join(" ")} failed`);
}

function contentType(name) {
  if (name === "SHA256SUMS") return "text/plain; charset=utf-8";
  if (name.endsWith(".json")) return "application/json; charset=utf-8";
  if (name.endsWith(".zip")) return "application/zip";
  if (name.endsWith(".tar.gz")) return "application/gzip";
  if (name.endsWith(".deb")) return "application/vnd.debian.binary-package";
  if (name.endsWith(".sh")) return "text/x-shellscript; charset=utf-8";
  return "application/octet-stream";
}

function upload(key, file, cacheControl, disposition) {
  const args = [
    "r2", "object", "put", `${BUCKET}/${key}`,
    "--remote",
    "--file", file,
    "--content-type", contentType(basename(file)),
    "--cache-control", cacheControl,
    "--force",
  ];
  if (disposition) args.push("--content-disposition", disposition);
  runWrangler(args);
}

async function publishRelease(args) {
  const version = normalizeVersion(args.version);
  const artifactsDir = resolve(args.artifacts_dir ?? "");
  const installer = resolve(args.installer ?? "");
  const manifest = await generateManifest({
    version,
    artifactsDir,
    publishedAt: args.published_at,
    notesUrl: args.notes_url,
    baseUrl: args.base_url,
  });
  const temporaryDirectory = await mkdtemp(join(tmpdir(), "workman-release-manifest-"));
  const manifestPath = join(temporaryDirectory, `${version}.json`);

  try {
    await writeFile(manifestPath, `${JSON.stringify(manifest, null, 2)}\n`, { mode: 0o600 });
    for (const asset of manifest.assets) {
      upload(
        `versions/${version}/${asset.name}`,
        join(artifactsDir, asset.name),
        "public, max-age=31536000, immutable",
        `attachment; filename=\"${asset.name}\"`,
      );
    }
    upload(
      `_manifests/${version}.json`,
      manifestPath,
      "public, max-age=31536000, immutable",
    );
    upload("install.sh", installer, "public, max-age=300, must-revalidate");
    upload("channels/latest.json", manifestPath, "public, max-age=60, must-revalidate");
  } finally {
    await rm(temporaryDirectory, { recursive: true, force: true });
  }
  process.stdout.write(`Published Workman ${version} artifacts and latest pointer to R2.\n`);
}

async function promoteRelease(args) {
  const version = normalizeVersion(args.version);
  const temporaryDirectory = await mkdtemp(join(tmpdir(), "workman-promote-manifest-"));
  const manifestPath = join(temporaryDirectory, `${version}.json`);
  try {
    runWrangler([
      "r2", "object", "get", `${BUCKET}/_manifests/${version}.json`,
      "--remote",
      "--file", manifestPath,
    ]);
    const manifest = JSON.parse(await readFile(manifestPath, "utf8"));
    if (manifest.version !== version || !Array.isArray(manifest.assets)) {
      throw new Error(`remote manifest for ${version} is invalid`);
    }
    upload("channels/stable.json", manifestPath, "public, max-age=60, must-revalidate");
  } finally {
    await rm(temporaryDirectory, { recursive: true, force: true });
  }
  process.stdout.write(`Promoted Workman ${version} to the R2 stable channel.\n`);
}

const [command, ...values] = process.argv.slice(2);
const args = parseArguments(values);
const action = command === "release"
  ? publishRelease(args)
  : command === "promote"
    ? promoteRelease(args)
    : Promise.reject(new Error("usage: publish-release.mjs release|promote [options]"));

action.catch((cause) => {
  process.stderr.write(`${cause instanceof Error ? cause.message : String(cause)}\n`);
  process.exitCode = 1;
});
