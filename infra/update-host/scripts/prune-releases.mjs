#!/usr/bin/env node

import { fileURLToPath } from "node:url";

const BUCKET_NAME = "workman-releases";
const VERSION = /^(\d+)\.(\d+)\.(\d+)$/;
const STORAGE_USD_PER_GB_MONTH = 0.015;
const REMOTE_CONFIG = fileURLToPath(new URL("../wrangler.prune.jsonc", import.meta.url));

function normalizeVersion(value, label = "version") {
  const match = VERSION.exec(value ?? "");
  if (match === null) throw new Error(`${label} must be X.Y.Z`);
  return match.slice(1).map(Number).join(".");
}

function compareVersions(left, right) {
  const leftParts = normalizeVersion(left).split(".").map(Number);
  const rightParts = normalizeVersion(right).split(".").map(Number);
  for (let index = 0; index < leftParts.length; index += 1) {
    if (leftParts[index] !== rightParts[index]) return leftParts[index] - rightParts[index];
  }
  return 0;
}

function releaseObject(key) {
  const asset = /^versions\/(\d+\.\d+\.\d+)\/(.+)$/.exec(key);
  if (asset !== null) return { version: normalizeVersion(asset[1]), kind: "asset" };
  const manifest = /^_manifests\/(\d+\.\d+\.\d+)\.json$/.exec(key);
  if (manifest !== null) return { version: normalizeVersion(manifest[1]), kind: "manifest" };
  return null;
}

function objectSize(object) {
  if (!Number.isSafeInteger(object.size) || object.size < 0) {
    throw new Error(`invalid size for R2 object ${object.key}`);
  }
  return object.size;
}

export function buildRetentionPlan({ objects, stableVersion, latestVersion }) {
  const stable = normalizeVersion(stableVersion, "stable channel version");
  const latest = normalizeVersion(latestVersion, "latest channel version");
  const releases = new Map();
  const unmanagedObjects = [];
  let totalBytes = 0;

  for (const object of objects) {
    if (typeof object.key !== "string" || object.key === "") {
      throw new Error("R2 inventory contains an object without a key");
    }
    totalBytes += objectSize(object);
    const release = releaseObject(object.key);
    if (release === null) {
      unmanagedObjects.push(object);
      continue;
    }
    const entry = releases.get(release.version) ?? {
      version: release.version,
      objects: [],
      hasAssets: false,
      hasManifest: false,
    };
    entry.objects.push(object);
    entry.hasAssets ||= release.kind === "asset";
    entry.hasManifest ||= release.kind === "manifest";
    releases.set(release.version, entry);
  }

  if (!releases.has(stable)) {
    throw new Error(`stable channel references ${stable}, but that version is absent from R2`);
  }
  if (!releases.has(latest)) {
    throw new Error(`latest channel references ${latest}, but that version is absent from R2`);
  }

  const versions = [...releases.keys()].sort(compareVersions);
  const priorStable = versions.filter((version) => compareVersions(version, stable) < 0).at(-1) ?? null;
  const keepReasons = new Map();
  const keep = (version, reason) => {
    const reasons = keepReasons.get(version) ?? [];
    if (!reasons.includes(reason)) reasons.push(reason);
    keepReasons.set(version, reasons);
  };
  keep(stable, "stable channel");
  keep(latest, "latest channel");
  if (priorStable !== null) keep(priorStable, "prior stable rollback");

  const describeRelease = (entry) => {
    entry.objects.sort((left, right) => left.key.localeCompare(right.key));
    const bytes = entry.objects.reduce((total, object) => total + objectSize(object), 0);
    const orphanReasons = [];
    if (!entry.hasAssets) orphanReasons.push("manifest has no version objects");
    if (!entry.hasManifest) orphanReasons.push("version objects have no manifest");
    return { ...entry, bytes, orphanReasons };
  };

  const keptVersions = versions
    .filter((version) => keepReasons.has(version))
    .map((version) => ({
      ...describeRelease(releases.get(version)),
      reasons: keepReasons.get(version),
    }));
  const deleteVersions = versions
    .filter((version) => !keepReasons.has(version))
    .map((version) => describeRelease(releases.get(version)));

  unmanagedObjects.sort((left, right) => left.key.localeCompare(right.key));
  return {
    stableVersion: stable,
    latestVersion: latest,
    priorStable,
    totalObjects: objects.length,
    totalBytes,
    keptVersions,
    deleteVersions,
    unmanagedObjects,
    deleteObjects: deleteVersions.flatMap(({ objects: versionObjects }) => versionObjects),
    deleteBytes: deleteVersions.reduce((total, version) => total + version.bytes, 0),
  };
}

function parseArguments(values) {
  let execute = false;
  let explicitDryRun = false;
  for (const value of values) {
    if (value === "--yes") execute = true;
    else if (value === "--dry-run") explicitDryRun = true;
    else if (value === "--help" || value === "-h") return { help: true, execute: false };
    else throw new Error(`unknown option: ${value}`);
  }
  if (execute && explicitDryRun) throw new Error("choose either --dry-run or --yes, not both");
  return { help: false, execute };
}

function formatBytes(bytes) {
  return `${bytes.toLocaleString("en-US")} bytes (${(bytes / 1_000_000).toFixed(2)} MB)`;
}

function monthlyStorageCost(bytes) {
  return (bytes / 1_000_000_000) * STORAGE_USD_PER_GB_MONTH;
}

async function readChannel(bucket, name) {
  const key = `channels/${name}.json`;
  const object = await bucket.get(key);
  if (object === null) throw new Error(`required channel object is missing: ${key}`);
  let manifest;
  try {
    manifest = await object.json();
  } catch (cause) {
    throw new Error(`${key} is not valid JSON`, { cause });
  }
  return normalizeVersion(manifest?.version, `${name} channel version`);
}

async function listObjects(bucket) {
  const objects = [];
  let cursor;
  do {
    const page = await bucket.list(cursor === undefined ? {} : { cursor });
    for (const object of page.objects) {
      objects.push({
        key: object.key,
        size: object.size,
        uploaded: object.uploaded,
        storageClass: object.storageClass,
      });
    }
    if (!page.truncated) break;
    if (typeof page.cursor !== "string" || page.cursor === "") {
      throw new Error("R2 returned a truncated object list without a cursor");
    }
    cursor = page.cursor;
  } while (true);
  return objects;
}

async function inspectBucket(bucket) {
  const [objects, stableVersion, latestVersion] = await Promise.all([
    listObjects(bucket),
    readChannel(bucket, "stable"),
    readChannel(bucket, "latest"),
  ]);
  return buildRetentionPlan({ objects, stableVersion, latestVersion });
}

function printPlan(plan, execute) {
  const mode = execute ? "EXECUTE" : "DRY RUN (default)";
  process.stdout.write(`R2 release retention plan — ${mode}\n`);
  process.stdout.write(`Bucket: ${BUCKET_NAME}\n`);
  process.stdout.write(`Channels: stable=${plan.stableVersion}, latest=${plan.latestVersion}\n`);
  process.stdout.write(`Before: ${plan.totalObjects} objects, ${formatBytes(plan.totalBytes)}\n`);
  process.stdout.write(
    `Nominal Standard storage: $${monthlyStorageCost(plan.totalBytes).toFixed(4)}/month `
      + "before the account-wide 10 GB-month free tier\n",
  );

  for (const version of plan.keptVersions) {
    process.stdout.write(
      `KEEP ${version.version}: ${version.reasons.join(", ")} — `
        + `${version.objects.length} objects, ${formatBytes(version.bytes)}\n`,
    );
  }
  for (const version of plan.deleteVersions) {
    const orphan = version.orphanReasons.length > 0
      ? `; orphan: ${version.orphanReasons.join(", ")}`
      : "";
    process.stdout.write(
      `DELETE ${version.version}: outside retention policy${orphan} — `
        + `${version.objects.length} objects, ${formatBytes(version.bytes)}\n`,
    );
    for (const object of version.objects) {
      process.stdout.write(`  DELETE ${object.key} — ${formatBytes(object.size)}\n`);
    }
  }
  process.stdout.write(
    `KEEP non-release objects: ${plan.unmanagedObjects.length} objects, `
      + `${formatBytes(plan.unmanagedObjects.reduce((total, object) => total + object.size, 0))}\n`,
  );
  for (const object of plan.unmanagedObjects) {
    process.stdout.write(`  KEEP ${object.key} — ${formatBytes(object.size)}\n`);
  }
  process.stdout.write(
    `Planned deletion: ${plan.deleteObjects.length} objects, ${formatBytes(plan.deleteBytes)}\n`,
  );
  if (!execute) process.stdout.write("No objects deleted. Re-run with --yes to execute this plan.\n");
}

async function executePlan(bucket, initialPlan) {
  let deletedObjects = 0;
  let deletedBytes = 0;
  for (const initialVersion of initialPlan.deleteVersions) {
    // Re-read both channels and the full inventory immediately before each version. This makes a
    // promotion/rollback between planning and execution fail closed, including the prior-stable guard.
    const currentPlan = await inspectBucket(bucket);
    const currentVersion = currentPlan.deleteVersions.find(
      ({ version }) => version === initialVersion.version,
    );
    if (currentVersion === undefined) {
      throw new Error(
        `retention guard changed: ${initialVersion.version} is no longer deletable; refusing to continue`,
      );
    }
    for (const object of currentVersion.objects) {
      await bucket.delete(object.key);
      deletedObjects += 1;
      deletedBytes += object.size;
      process.stdout.write(`DELETED ${object.key} — ${formatBytes(object.size)}\n`);
    }
  }
  const after = await inspectBucket(bucket);
  process.stdout.write(`After: ${after.totalObjects} objects, ${formatBytes(after.totalBytes)}\n`);
  process.stdout.write(
    `Reclaimed: ${deletedObjects} objects, ${formatBytes(deletedBytes)}; `
      + `nominal storage reduction $${monthlyStorageCost(deletedBytes).toFixed(4)}/month\n`,
  );
  return { after, deletedObjects, deletedBytes };
}

function usage() {
  process.stdout.write(`Usage: npm run prune -- [--dry-run | --yes]\n
Lists the production ${BUCKET_NAME} bucket and applies channel-aware release retention.
The default is --dry-run. --yes permanently deletes only versions outside the policy.\n`);
}

async function main() {
  const args = parseArguments(process.argv.slice(2));
  if (args.help) {
    usage();
    return;
  }
  const { getPlatformProxy } = await import("wrangler");
  const proxy = await getPlatformProxy({
    configPath: REMOTE_CONFIG,
    persist: false,
    remoteBindings: true,
  });
  try {
    const bucket = proxy.env.RELEASES;
    if (bucket === undefined) throw new Error("remote RELEASES binding is unavailable");
    const plan = await inspectBucket(bucket);
    printPlan(plan, args.execute);
    if (args.execute) await executePlan(bucket, plan);
  } finally {
    await proxy.dispose();
  }
}

if (process.argv[1] === fileURLToPath(import.meta.url)) {
  main().catch((cause) => {
    process.stderr.write(`${cause instanceof Error ? cause.message : String(cause)}\n`);
    process.exitCode = 1;
  });
}
