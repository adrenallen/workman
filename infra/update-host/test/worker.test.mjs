import assert from "node:assert/strict";
import test from "node:test";
import worker from "../src/index.ts";

const content = Uint8Array.from({ length: 64 }, (_, index) => index);

function object(body = content) {
  return {
    body: new ReadableStream({
      start(controller) {
        controller.enqueue(body);
        controller.close();
      },
    }),
    httpEtag: '"fixture-etag"',
    size: content.byteLength,
    writeHttpMetadata(headers) {
      headers.set("content-type", "application/octet-stream");
    },
  };
}

function rangedBody(range) {
  if (range === undefined) return content;
  if ("suffix" in range) return content.slice(-range.suffix);
  const offset = range.offset ?? 0;
  return content.slice(offset, offset + (range.length ?? content.byteLength));
}

const env = {
  RELEASES: {
    async get(key, options = {}) {
      if (key !== "versions/1.2.3/fixture.zip") return null;
      return object(rangedBody(options.range));
    },
    async head(key) {
      return key === "versions/1.2.3/fixture.zip" ? object() : null;
    },
  },
};

test("serves full artifacts as 200 without a Content-Range", async () => {
  const response = await worker.fetch(
    new Request("https://workman.userdefined.io/versions/1.2.3/fixture.zip"),
    env,
  );
  assert.equal(response.status, 200);
  assert.equal(response.headers.get("content-length"), "64");
  assert.equal(response.headers.get("content-range"), null);
  assert.equal((await response.arrayBuffer()).byteLength, 64);
});

test("serves a single byte range with exact response headers", async () => {
  const response = await worker.fetch(
    new Request("https://workman.userdefined.io/versions/1.2.3/fixture.zip", {
      headers: { range: "bytes=8-23" },
    }),
    env,
  );
  assert.equal(response.status, 206);
  assert.equal(response.headers.get("content-length"), "16");
  assert.equal(response.headers.get("content-range"), "bytes 8-23/64");
  assert.deepEqual(new Uint8Array(await response.arrayBuffer()), content.slice(8, 24));
});

test("rejects malformed and unsatisfiable byte ranges", async () => {
  for (const range of ["bytes=20-10", "bytes=80-"]) {
    const response = await worker.fetch(
      new Request("https://workman.userdefined.io/versions/1.2.3/fixture.zip", {
        headers: { range },
      }),
      env,
    );
    assert.equal(response.status, 416);
  }
});
