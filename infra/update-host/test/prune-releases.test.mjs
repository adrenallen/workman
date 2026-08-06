import assert from "node:assert/strict";
import test from "node:test";
import { buildRetentionPlan } from "../scripts/prune-releases.mjs";

function object(key, size = 100) {
  return { key, size };
}

function release(version, { assets = true, manifest = true } = {}) {
  const objects = [];
  if (assets) objects.push(object(`versions/${version}/workman.zip`));
  if (manifest) objects.push(object(`_manifests/${version}.json`, 20));
  return objects;
}

test("keeps both channel versions and exactly one prior stable", () => {
  const plan = buildRetentionPlan({
    objects: [
      ...release("0.1.0"),
      ...release("0.1.1"),
      ...release("0.1.2"),
      ...release("0.1.3"),
      object("channels/stable.json", 30),
      object("channels/latest.json", 30),
    ],
    stableVersion: "0.1.2",
    latestVersion: "0.1.3",
  });

  assert.equal(plan.priorStable, "0.1.1");
  assert.deepEqual(plan.keptVersions.map(({ version }) => version), ["0.1.1", "0.1.2", "0.1.3"]);
  assert.deepEqual(plan.deleteVersions.map(({ version }) => version), ["0.1.0"]);
  assert.ok(plan.keptVersions.find(({ version }) => version === "0.1.2").reasons.includes("stable channel"));
  assert.ok(plan.keptVersions.find(({ version }) => version === "0.1.3").reasons.includes("latest channel"));
});

test("never schedules a channel-referenced version even when both channels match", () => {
  const plan = buildRetentionPlan({
    objects: [...release("1.9.9"), ...release("1.10.0"), ...release("2.0.0")],
    stableVersion: "2.0.0",
    latestVersion: "2.0.0",
  });

  assert.deepEqual(plan.keptVersions.map(({ version }) => version), ["1.10.0", "2.0.0"]);
  assert.deepEqual(plan.deleteVersions.map(({ version }) => version), ["1.9.9"]);
  assert.equal(
    plan.deleteObjects.some(({ key }) => key.includes("2.0.0")),
    false,
  );
});

test("detects valid-version orphan objects and manifests", () => {
  const plan = buildRetentionPlan({
    objects: [
      ...release("2.0.0"),
      ...release("1.9.0"),
      ...release("1.8.0", { manifest: false }),
      ...release("1.7.0", { assets: false }),
      object("branding/logo.png", 50),
      object("versions/not-a-version/leave-me", 60),
    ],
    stableVersion: "2.0.0",
    latestVersion: "2.0.0",
  });

  assert.equal(plan.priorStable, "1.9.0");
  assert.deepEqual(plan.deleteVersions.map(({ version }) => version), ["1.7.0", "1.8.0"]);
  assert.deepEqual(plan.deleteVersions[0].orphanReasons, ["manifest has no version objects"]);
  assert.deepEqual(plan.deleteVersions[1].orphanReasons, ["version objects have no manifest"]);
  assert.deepEqual(plan.unmanagedObjects.map(({ key }) => key), [
    "branding/logo.png",
    "versions/not-a-version/leave-me",
  ]);
});

test("fails closed when a channel target is absent", () => {
  assert.throws(
    () => buildRetentionPlan({
      objects: release("1.2.2"),
      stableVersion: "1.2.3",
      latestVersion: "1.2.2",
    }),
    /stable channel references 1\.2\.3, but that version is absent/,
  );
});
