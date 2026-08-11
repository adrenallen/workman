import assert from "node:assert/strict";
import test from "node:test";
import worker from "../src/index.ts";

const artifact = Uint8Array.from({ length: 64 }, (_, index) => index);
const logo = new TextEncoder().encode("fixture-logo");
const installer = new TextEncoder().encode("#!/usr/bin/env bash\necho installer\n");
const basicAuthorization = `Basic ${Buffer.from("friend:friend-key").toString("base64")}`;
function releaseAsset(name, target, size) {
  return {
    name,
    target,
    sha256: "0".repeat(64),
    size,
    url: `https://workman.userdefined.io/versions/1.2.3/${name}`,
  };
}

const release = {
  version: "1.2.3",
  published_at: "2026-08-06T12:34:56.000Z",
  notes_url: "https://example.com/releases/1.2.3",
  assets: [
    releaseAsset("fixture.zip", "fixture", artifact.byteLength),
    releaseAsset("workman-macos-arm64.zip", "macos-arm64", 80 * 1024 * 1024),
    releaseAsset("workman-linux-x86_64.AppImage", "linux-x86_64-appimage", 91 * 1024 * 1024),
    releaseAsset("workman-linux-x86_64.deb", "linux-x86_64-deb", 72 * 1024 * 1024),
    releaseAsset("workman-linux-x86_64.tar.gz", "linux-x86_64", 70 * 1024 * 1024),
    releaseAsset("workman-linux-arm64.AppImage", "linux-arm64-appimage", 88 * 1024 * 1024),
    releaseAsset("workman-linux-arm64.deb", "linux-arm64-deb", 69 * 1024 * 1024),
    releaseAsset("workman-linux-arm64.tar.gz", "linux-arm64", 67 * 1024 * 1024),
    releaseAsset("SHA256SUMS", "checksums", 812),
  ],
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
  if (key.startsWith("versions/1.2.3/") && release.assets.some((asset) => key.endsWith(`/${asset.name}`))) {
    return [artifact, "application/octet-stream"];
  }
  if (key === "branding/workman-logo-wide-transparent.png") return [logo, "image/png"];
  if (key === "install.sh") return [installer, "text/x-shellscript; charset=utf-8"];
  if (key === "channels/stable.json" || key === "channels/latest.json") {
    return [new TextEncoder().encode(JSON.stringify(release)), "application/json; charset=utf-8"];
  }
  return null;
}

const env = {
  DOWNLOAD_KEYS: "app-key, friend-key",
  SITE_NOINDEX: "true",
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
  assert.match(html, /href="\/download"/);
  assert.match(html, /<meta name="robots" content="noindex">/);
  assert.equal(lander.headers.get("x-robots-tag"), "noindex, nofollow");

  const image = await worker.fetch(request("/workman-logo-wide-transparent.png"), env);
  assert.equal(image.status, 200);
  assert.equal(image.headers.get("content-type"), "image/png");
  assert.equal(image.headers.get("cache-control"), "public, max-age=31536000, immutable");
  assert.equal(image.headers.get("x-robots-tag"), "noindex, nofollow");
  assert.deepEqual(new Uint8Array(await image.arrayBuffer()), logo);
});

test("renders the Basic-authenticated stable download page entirely from its channel manifest", async () => {
  const response = await worker.fetch(
    request("/download", { headers: { authorization: basicAuthorization } }),
    env,
  );
  assert.equal(response.status, 200);
  assert.equal(response.headers.get("cache-control"), "no-store");
  assert.match(response.headers.get("content-security-policy"), /default-src 'none'/);
  assert.equal(response.headers.get("x-robots-tag"), "noindex, nofollow");
  const html = await response.text();

  assert.match(html, /<meta name="robots" content="noindex">/);
  assert.match(html, /Current stable/);
  assert.match(html, /Workman <span>v1\.2\.3<\/span>/);
  assert.match(html, /Aug 6, 2026/);
  assert.match(html, /Access granted/);
  assert.match(html, /reuse this browser session's access credentials/);
  assert.match(html, /href="\/versions\/1\.2\.3\/workman-macos-arm64\.zip"/);
  assert.match(html, /href="\/versions\/1\.2\.3\/workman-linux-x86_64\.AppImage"/);
  assert.match(html, /href="\/versions\/1\.2\.3\/workman-linux-arm64\.deb"/);
  assert.match(html, /href="\/versions\/1\.2\.3\/SHA256SUMS"/);
  assert.match(html, /80 MB/);
  assert.match(html, /WORKMAN_KEY='your-password'/);
  assert.match(html, /First launch on macOS/);
  assert.match(html, /Developer ID signed and notarized/);
  assert.match(html, /should pass Gatekeeper and open normally/);
  assert.match(html, /Versions 0\.1\.4 and earlier were unsigned/);
  assert.doesNotMatch(html, /xattr -dr com\.apple\.quarantine/);
  assert.doesNotMatch(html, /app-key|friend-key/);

  const head = await worker.fetch(
    request("/download", { method: "HEAD", headers: { authorization: basicAuthorization } }),
    env,
  );
  assert.equal(head.status, 200);
  assert.equal(await head.text(), "");
});

test("requires interactive Basic auth for the download page regardless of URL or API credentials", async () => {
  for (const [path, headers] of [
    ["/download", {}],
    ["/download?key=friend-key", {}],
    ["/download", { authorization: "Bearer app-key" }],
    ["/download", { "x-workman-key": "friend-key" }],
  ]) {
    const response = await worker.fetch(request(path, { headers }), env);
    assert.equal(response.status, 401, `${path} ${JSON.stringify(headers)}`);
    assert.equal(response.headers.get("www-authenticate"), 'Basic realm="workman"');
    assert.equal(response.headers.get("content-type"), "text/plain; charset=utf-8");
    assert.doesNotMatch(await response.text(), /<title>Download Workman/);
  }

  const wrongPassword = await worker.fetch(
    request("/download", {
      headers: { authorization: `Basic ${Buffer.from("anyone:wrong-key").toString("base64")}` },
    }),
    env,
  );
  assert.equal(wrongPassword.status, 401);
  assert.equal(wrongPassword.headers.get("www-authenticate"), 'Basic realm="workman"');
});

test("keeps the Gatekeeper workaround on legacy unsigned release pages", async () => {
  const legacyRelease = { ...release, version: "0.1.4" };
  const legacyEnv = {
    ...env,
    RELEASES: {
      ...env.RELEASES,
      async get(key, options = {}) {
        if (key === "channels/stable.json") {
          const body = new TextEncoder().encode(JSON.stringify(legacyRelease));
          return object(rangedBody(body, options.range), "application/json; charset=utf-8", body.byteLength);
        }
        return env.RELEASES.get(key, options);
      },
    },
  };

  const response = await worker.fetch(
    request("/download", { headers: { authorization: basicAuthorization } }),
    legacyEnv,
  );
  assert.equal(response.status, 200);
  const html = await response.text();
  assert.match(html, /Gatekeeper blocks the unsigned app/);
  assert.match(html, /xattr -dr com\.apple\.quarantine \/Applications\/Workman\.app/);
  assert.match(html, /System Settings &rarr; Privacy &amp; Security/);
  assert.match(html, /CLI installer path does not apply browser quarantine/);
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

  const secondAsset = await worker.fetch(
    request("/versions/1.2.3/workman-linux-x86_64.AppImage", { headers: { accept: "text/html" } }),
    env,
  );
  assert.equal(secondAsset.status, 401);
  assert.equal(secondAsset.headers.get("www-authenticate"), 'Basic realm="workman"');
});

test("accepts Bearer, X-Workman-Key, and Basic credentials for artifacts", async () => {
  const mechanisms = [
    { headers: { authorization: "Bearer app-key" } },
    { headers: { "x-workman-key": "friend-key" } },
    { headers: { authorization: basicAuthorization } },
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

test("does not accept a URL query key for protected artifacts", async () => {
  const response = await worker.fetch(request("/versions/1.2.3/fixture.zip?key=app-key"), env);
  assert.equal(response.status, 401);
  assert.deepEqual(await response.json(), { error: "invalid or missing download key" });
});

test("keeps Bearer and X-Workman-Key manifest authorization compatible", async () => {
  for (const headers of [
    { authorization: "Bearer app-key" },
    { "x-workman-key": "app-key" },
  ]) {
    const response = await worker.fetch(request("/releases.json", { headers }), env);
    assert.equal(response.status, 200);
    assert.deepEqual(await response.json(), { channels: { stable: release, latest: release } });
  }
});

test("keeps the bootstrap installer public", async () => {
  const response = await worker.fetch(request("/install.sh"), env);
  assert.equal(response.status, 200);
  assert.equal(response.headers.get("content-type"), "text/x-shellscript; charset=utf-8");
  assert.equal(response.headers.get("x-robots-tag"), "noindex, nofollow");
  assert.deepEqual(new Uint8Array(await response.arrayBuffer()), installer);
});

test("serves robots.txt and applies noindex to every response class", async () => {
  const robots = await worker.fetch(request("/robots.txt"), env);
  assert.equal(robots.status, 200);
  assert.equal(robots.headers.get("content-type"), "text/plain; charset=utf-8");
  assert.equal(await robots.text(), "User-agent: *\nDisallow: /\n");

  const cases = [
    request("/"),
    request("/download"),
    request("/download", { headers: { authorization: basicAuthorization } }),
    request("/robots.txt"),
    request("/workman-logo-wide-transparent.png"),
    request("/install.sh"),
    request("/releases.json", { headers: { authorization: "Bearer app-key" } }),
    request("/versions/1.2.3/fixture.zip", { headers: { authorization: "Bearer app-key" } }),
    request("/missing"),
    request("/", { method: "POST" }),
  ];
  for (const item of cases) {
    const response = await worker.fetch(item, env);
    assert.equal(
      response.headers.get("x-robots-tag"),
      "noindex, nofollow",
      `${item.method} ${new URL(item.url).pathname}`,
    );
  }
});

test("disables every search-engine block with the single config flag", async () => {
  const indexingEnv = { ...env, SITE_NOINDEX: "false" };
  const lander = await worker.fetch(request("/"), indexingEnv);
  assert.equal(lander.headers.get("x-robots-tag"), null);
  assert.doesNotMatch(await lander.text(), /<meta name="robots"/);

  const download = await worker.fetch(
    request("/download", { headers: { authorization: basicAuthorization } }),
    indexingEnv,
  );
  assert.equal(download.headers.get("x-robots-tag"), null);
  assert.doesNotMatch(await download.text(), /<meta name="robots"/);

  const robots = await worker.fetch(request("/robots.txt"), indexingEnv);
  assert.equal(robots.headers.get("x-robots-tag"), null);
  assert.equal(await robots.text(), "User-agent: *\nAllow: /\n");
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
