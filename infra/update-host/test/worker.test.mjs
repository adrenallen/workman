import assert from "node:assert/strict";
import test from "node:test";
import worker from "../src/index.ts";

const artifact = Uint8Array.from({ length: 64 }, (_, index) => index);
const logo = new TextEncoder().encode("fixture-logo");
const installer = new TextEncoder().encode("#!/usr/bin/env bash\necho installer\n");
const release = {
  version: "1.2.3",
  published_at: "2026-08-06T12:34:56.000Z",
  notes_url: "https://example.com/releases/1.2.3",
  assets: [{
    name: "fixture.zip",
    target: "fixture",
    sha256: "0".repeat(64),
    size: artifact.byteLength,
    url: "https://workman.userdefined.io/versions/1.2.3/fixture.zip",
  }],
};

function object(body, contentType = "application/octet-stream", size = body.byteLength) {
  return {
    body: new ReadableStream({
      start(controller) {
        controller.enqueue(body);
        controller.close();
      },
    }),
    httpEtag: '"fixture-etag"',
    size,
    writeHttpMetadata(headers) {
      headers.set("content-type", contentType);
    },
    async json() {
      return JSON.parse(new TextDecoder().decode(body));
    },
  };
}

function rangedBody(body, range) {
  if (range === undefined) return body;
  if ("suffix" in range) return body.slice(-range.suffix);
  const offset = range.offset ?? 0;
  return body.slice(offset, offset + (range.length ?? body.byteLength));
}

function storedObject(key) {
  if (key === "versions/1.2.3/fixture.zip") return [artifact, "application/zip"];
  if (key === "branding/workman-logo-wide-transparent.png") return [logo, "image/png"];
  if (key === "install.sh") return [installer, "text/x-shellscript; charset=utf-8"];
  if (key === "channels/stable.json" || key === "channels/latest.json") {
    return [new TextEncoder().encode(JSON.stringify(release)), "application/json; charset=utf-8"];
  }
  return null;
}

const env = {
  DOWNLOAD_KEYS: "app-key, friend-key",
  RELEASES: {
    async get(key, options = {}) {
      const stored = storedObject(key);
      if (stored === null) return null;
      const [body, contentType] = stored;
      return object(rangedBody(body, options.range), contentType, body.byteLength);
    },
    async head(key) {
      const stored = storedObject(key);
      return stored === null ? null : object(...stored);
    },
  },
};

function request(path, init) {
  return new Request(`https://workman.userdefined.io${path}`, init);
}

test("keeps the black logo lander and its R2 image public", async () => {
  const lander = await worker.fetch(request("/"), env);
  assert.equal(lander.status, 200);
  assert.match(lander.headers.get("content-type"), /^text\/html/);
  const html = await lander.text();
  assert.match(html, /<title>Workman<\/title>/);
  assert.match(html, /background: #000/);
  assert.match(html, /src="\/workman-logo-wide-transparent\.png"/);

  const image = await worker.fetch(request("/workman-logo-wide-transparent.png"), env);
  assert.equal(image.status, 200);
  assert.equal(image.headers.get("content-type"), "image/png");
  assert.equal(image.headers.get("cache-control"), "public, max-age=31536000, immutable");
  assert.deepEqual(new Uint8Array(await image.arrayBuffer()), logo);
});

test("returns contract-specific 401 responses before reading protected objects", async () => {
  const api = await worker.fetch(
    request("/releases.json", { headers: { accept: "application/json" } }),
    env,
  );
  assert.equal(api.status, 401);
  assert.equal(api.headers.get("www-authenticate"), null);
  assert.deepEqual(await api.json(), { error: "invalid or missing download key" });

  const browser = await worker.fetch(
    request("/versions/1.2.3/fixture.zip", { headers: { accept: "text/html,application/xhtml+xml" } }),
    env,
  );
  assert.equal(browser.status, 401);
  assert.equal(browser.headers.get("www-authenticate"), 'Basic realm="workman"');
  assert.match(await browser.text(), /download key is required/i);
});

test("accepts Bearer, X-Workman-Key, query, and Basic credentials", async () => {
  const mechanisms = [
    { headers: { authorization: "Bearer app-key" } },
    { headers: { "x-workman-key": "friend-key" } },
    { path: "/versions/1.2.3/fixture.zip?key=app-key" },
    { headers: { authorization: `Basic ${Buffer.from("friend:friend-key").toString("base64")}` } },
  ];

  for (const mechanism of mechanisms) {
    const response = await worker.fetch(
      request(mechanism.path ?? "/versions/1.2.3/fixture.zip", { headers: mechanism.headers }),
      env,
    );
    assert.equal(response.status, 200);
    assert.deepEqual(new Uint8Array(await response.arrayBuffer()), artifact);
  }
});

test("serves a protected manifest with an authorized channel response", async () => {
  const response = await worker.fetch(
    request("/releases.json", { headers: { authorization: "Bearer app-key" } }),
    env,
  );
  assert.equal(response.status, 200);
  assert.deepEqual(await response.json(), { channels: { stable: release, latest: release } });
});

test("keeps the bootstrap installer public", async () => {
  const response = await worker.fetch(request("/install.sh"), env);
  assert.equal(response.status, 200);
  assert.equal(response.headers.get("content-type"), "text/x-shellscript; charset=utf-8");
  assert.deepEqual(new Uint8Array(await response.arrayBuffer()), installer);
});

test("serves authorized byte ranges with exact response headers", async () => {
  const response = await worker.fetch(
    request("/versions/1.2.3/fixture.zip", {
      headers: { authorization: "Bearer app-key", range: "bytes=8-23" },
    }),
    env,
  );
  assert.equal(response.status, 206);
  assert.equal(response.headers.get("content-length"), "16");
  assert.equal(response.headers.get("content-range"), "bytes 8-23/64");
  assert.deepEqual(new Uint8Array(await response.arrayBuffer()), artifact.slice(8, 24));
});

test("rejects malformed and unsatisfiable authorized byte ranges", async () => {
  for (const range of ["bytes=20-10", "bytes=80-"]) {
    const response = await worker.fetch(
      request("/versions/1.2.3/fixture.zip", {
        headers: { authorization: "Bearer app-key", range },
      }),
      env,
    );
    assert.equal(response.status, 416);
  }
});
