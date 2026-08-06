const VERSIONED_PATH = /^\/versions\/(\d+\.\d+\.\d+)\/([A-Za-z0-9][A-Za-z0-9._-]*)$/;
const IMMUTABLE_CACHE = "public, max-age=31536000, immutable";
const MANIFEST_CACHE = "public, max-age=60, must-revalidate";
const INSTALLER_CACHE = "public, max-age=300, must-revalidate";

interface ReleaseAsset {
  name: string;
  target: string;
  sha256: string;
  size: number;
  url: string;
}

interface ReleaseManifest {
  version: string;
  published_at: string;
  notes_url: string;
  assets: ReleaseAsset[];
}

function json(data: unknown, status = 200, cacheControl = "no-store"): Response {
  return Response.json(data, {
    status,
    headers: {
      "cache-control": cacheControl,
      "content-type": "application/json; charset=utf-8",
      "x-content-type-options": "nosniff",
    },
  });
}

function errorResponse(status: number, message: string): Response {
  return json({ error: message }, status);
}

function isReleaseManifest(value: unknown): value is ReleaseManifest {
  if (typeof value !== "object" || value === null) return false;
  const candidate = value as Partial<ReleaseManifest>;
  return (
    typeof candidate.version === "string" &&
    typeof candidate.published_at === "string" &&
    typeof candidate.notes_url === "string" &&
    Array.isArray(candidate.assets)
  );
}

async function readChannel(env: Env, channel: "stable" | "latest"): Promise<ReleaseManifest> {
  const object = await env.RELEASES.get(`channels/${channel}.json`);
  if (object === null) throw new Error(`${channel} channel pointer is missing`);
  const manifest: unknown = await object.json();
  if (!isReleaseManifest(manifest)) throw new Error(`${channel} channel pointer is invalid`);
  return manifest;
}

async function serveManifest(request: Request, env: Env): Promise<Response> {
  try {
    const [stable, latest] = await Promise.all([
      readChannel(env, "stable"),
      readChannel(env, "latest"),
    ]);
    const response = json({ channels: { stable, latest } }, 200, MANIFEST_CACHE);
    return request.method === "HEAD" ? new Response(null, response) : response;
  } catch (cause) {
    console.error(JSON.stringify({
      event: "release_manifest_unavailable",
      message: cause instanceof Error ? cause.message : String(cause),
    }));
    return errorResponse(503, "release manifest is temporarily unavailable");
  }
}

function requestedRange(headers: Headers): R2Range | undefined {
  const header = headers.get("range");
  if (header === null) return undefined;
  const match = /^bytes=(\d*)-(\d*)$/.exec(header);
  if (match === null || (match[1] === "" && match[2] === "")) {
    throw new RangeError("invalid byte range");
  }
  if (match[1] === "") {
    const suffix = Number(match[2]);
    if (!Number.isSafeInteger(suffix) || suffix <= 0) throw new RangeError("invalid byte range");
    return { suffix };
  }
  const offset = Number(match[1]);
  if (!Number.isSafeInteger(offset)) throw new RangeError("invalid byte range");
  if (match[2] === "") return { offset };
  const end = Number(match[2]);
  if (!Number.isSafeInteger(end) || end < offset) throw new RangeError("invalid byte range");
  return { offset, length: end - offset + 1 };
}

function rangeHeaders(object: R2ObjectBody, range: R2Range, headers: Headers): void {
  const { offset, length } = "suffix" in range
    ? {
        offset: Math.max(0, object.size - range.suffix),
        length: Math.min(object.size, range.suffix),
      }
    : {
        offset: range.offset ?? 0,
        length: Math.min(range.length ?? object.size, object.size - (range.offset ?? 0)),
      };
  if (offset >= object.size || length <= 0) throw new RangeError("unsatisfiable byte range");
  headers.set("content-length", String(length));
  headers.set("content-range", `bytes ${offset}-${offset + length - 1}/${object.size}`);
}

async function serveObject(
  request: Request,
  env: Env,
  key: string,
  cacheControl: string,
): Promise<Response> {
  try {
    if (request.method === "HEAD") {
      const object = await env.RELEASES.head(key);
      if (object === null) return errorResponse(404, "not found");
      const headers = new Headers();
      object.writeHttpMetadata(headers);
      headers.set("cache-control", cacheControl);
      headers.set("content-length", String(object.size));
      headers.set("etag", object.httpEtag);
      headers.set("x-content-type-options", "nosniff");
      return new Response(null, { status: 200, headers });
    }

    const range = requestedRange(request.headers);
    const options: R2GetOptions = range === undefined ? {} : { range };
    const object = await env.RELEASES.get(key, options);
    if (object === null || !("body" in object)) return errorResponse(404, "not found");

    const headers = new Headers();
    object.writeHttpMetadata(headers);
    headers.set("accept-ranges", "bytes");
    headers.set("cache-control", cacheControl);
    headers.set("content-length", String(object.size));
    headers.set("etag", object.httpEtag);
    headers.set("x-content-type-options", "nosniff");
    const status = range === undefined ? 200 : 206;
    if (range !== undefined) rangeHeaders(object, range, headers);
    return new Response(object.body, { status, headers });
  } catch (cause) {
    if (request.headers.has("range")) return errorResponse(416, "invalid byte range");
    console.error(JSON.stringify({
      event: "release_object_failed",
      key,
      message: cause instanceof Error ? cause.message : String(cause),
    }));
    return errorResponse(500, "artifact is temporarily unavailable");
  }
}

export default {
  async fetch(request, env): Promise<Response> {
    if (request.method !== "GET" && request.method !== "HEAD") {
      return new Response("Method Not Allowed", {
        status: 405,
        headers: { allow: "GET, HEAD", "cache-control": "no-store" },
      });
    }

    const pathname = new URL(request.url).pathname;
    if (pathname === "/releases.json") return serveManifest(request, env);
    if (pathname === "/install.sh") {
      return serveObject(request, env, "install.sh", INSTALLER_CACHE);
    }

    const versioned = VERSIONED_PATH.exec(pathname);
    if (versioned !== null) {
      const [, version, asset] = versioned;
      return serveObject(request, env, `versions/${version}/${asset}`, IMMUTABLE_CACHE);
    }

    return errorResponse(404, "not found");
  },
} satisfies ExportedHandler<Env>;
