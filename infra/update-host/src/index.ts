const VERSIONED_PATH = /^\/versions\/(\d+\.\d+\.\d+)\/([A-Za-z0-9][A-Za-z0-9._-]*)$/;
const IMMUTABLE_CACHE = "public, max-age=31536000, immutable";
const MANIFEST_CACHE = "public, max-age=60, must-revalidate";
const INSTALLER_CACHE = "public, max-age=300, must-revalidate";
const LOGO_PATH = "/workman-logo-wide-transparent.png";
const LOGO_KEY = "branding/workman-logo-wide-transparent.png";

type WorkerEnv = Env & {
  DOWNLOAD_KEYS: string;
};

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

function candidateKeys(request: Request): string[] {
  const candidates: string[] = [];
  const authorization = request.headers.get("authorization");
  if (authorization !== null) {
    const bearer = /^Bearer\s+(.+)$/i.exec(authorization);
    if (bearer !== null) candidates.push(bearer[1].trim());

    const basic = /^Basic\s+(.+)$/i.exec(authorization);
    if (basic !== null) {
      try {
        const decoded = atob(basic[1]);
        const separator = decoded.indexOf(":");
        if (separator !== -1) candidates.push(decoded.slice(separator + 1));
      } catch {
        // A malformed Basic credential is simply not a valid download key.
      }
    }
  }

  const headerKey = request.headers.get("x-workman-key");
  if (headerKey !== null) candidates.push(headerKey.trim());

  const queryKey = new URL(request.url).searchParams.get("key");
  if (queryKey !== null) candidates.push(queryKey);
  return candidates;
}

function isAuthorized(request: Request, env: WorkerEnv): boolean {
  const validKeys = new Set(
    env.DOWNLOAD_KEYS.split(",")
      .map((key) => key.trim())
      .filter((key) => key.length > 0),
  );
  return validKeys.size > 0 && candidateKeys(request).some((key) => validKeys.has(key));
}

function unauthorized(request: Request): Response {
  const browserRequest = request.headers.get("accept")?.toLowerCase().includes("text/html") ?? false;
  const response = browserRequest
    ? new Response("A Workman download key is required.\n", {
        status: 401,
        headers: {
          "content-type": "text/plain; charset=utf-8",
          "www-authenticate": 'Basic realm="workman"',
        },
      })
    : errorResponse(401, "invalid or missing download key");
  response.headers.set("cache-control", "no-store");
  return request.method === "HEAD" ? new Response(null, response) : response;
}

function serveLander(request: Request): Response {
  const html = `<!doctype html>
<html lang="en">
<head>
  <meta charset="utf-8">
  <meta name="viewport" content="width=device-width, initial-scale=1">
  <title>Workman</title>
  <style>
    html, body { width: 100%; height: 100%; margin: 0; background: #000; }
    body { display: grid; place-items: center; }
    img { display: block; width: min(90vw, 960px); height: auto; }
  </style>
</head>
<body>
  <img src="${LOGO_PATH}" alt="Workman">
</body>
</html>`;
  const response = new Response(html, {
    headers: {
      "cache-control": "public, max-age=300, must-revalidate",
      "content-security-policy": "default-src 'none'; img-src 'self'; style-src 'unsafe-inline'; base-uri 'none'; frame-ancestors 'none'",
      "content-type": "text/html; charset=utf-8",
      "referrer-policy": "no-referrer",
      "x-content-type-options": "nosniff",
    },
  });
  return request.method === "HEAD" ? new Response(null, response) : response;
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

async function readChannel(env: WorkerEnv, channel: "stable" | "latest"): Promise<ReleaseManifest> {
  const object = await env.RELEASES.get(`channels/${channel}.json`);
  if (object === null) throw new Error(`${channel} channel pointer is missing`);
  const manifest: unknown = await object.json();
  if (!isReleaseManifest(manifest)) throw new Error(`${channel} channel pointer is invalid`);
  return manifest;
}

async function serveManifest(request: Request, env: WorkerEnv): Promise<Response> {
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
  env: WorkerEnv,
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
    if (pathname === "/") return serveLander(request);
    if (pathname === LOGO_PATH) {
      return serveObject(request, env, LOGO_KEY, IMMUTABLE_CACHE);
    }

    if (
      (pathname === "/releases.json" || pathname.startsWith("/versions/")) &&
      !isAuthorized(request, env)
    ) {
      return unauthorized(request);
    }

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
} satisfies ExportedHandler<WorkerEnv>;
