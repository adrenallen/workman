const VERSIONED_PATH = /^\/versions\/(\d+\.\d+\.\d+)\/([A-Za-z0-9][A-Za-z0-9._-]*)$/;
const IMMUTABLE_CACHE = "public, max-age=31536000, immutable";
const MANIFEST_CACHE = "public, max-age=60, must-revalidate";
const INSTALLER_CACHE = "public, max-age=300, must-revalidate";
const PAGE_CACHE = "public, max-age=60, must-revalidate";
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

const DOWNLOAD_GROUPS = [
  {
    id: "macos-arm64",
    platform: "macOS",
    architecture: "Apple silicon",
    description: "Workman.app, wrk, and workmand",
    assets: [{ target: "macos-arm64", label: "Download for macOS", format: "ZIP" }],
  },
  {
    id: "linux-x86-64",
    platform: "Linux",
    architecture: "x86_64",
    description: "Choose the package for your distribution",
    assets: [
      { target: "linux-x86_64-appimage", label: "AppImage", format: "APPIMAGE" },
      { target: "linux-x86_64-deb", label: "Debian / Ubuntu", format: "DEB" },
      { target: "linux-x86_64", label: "Portable archive", format: "TAR.GZ" },
    ],
  },
  {
    id: "linux-arm64",
    platform: "Linux",
    architecture: "arm64",
    description: "Choose the package for your distribution",
    assets: [
      { target: "linux-arm64-appimage", label: "AppImage", format: "APPIMAGE" },
      { target: "linux-arm64-deb", label: "Debian / Ubuntu", format: "DEB" },
      { target: "linux-arm64", label: "Portable archive", format: "TAR.GZ" },
    ],
  },
] as const;

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

function escapeHtml(value: string): string {
  return value.replace(/[&<>'"]/g, (character) => ({
    "&": "&amp;",
    "<": "&lt;",
    ">": "&gt;",
    "'": "&#39;",
    '"': "&quot;",
  })[character] ?? character);
}

function pageResponse(request: Request, html: string, cacheControl = PAGE_CACHE): Response {
  const response = new Response(html, {
    headers: {
      "cache-control": cacheControl,
      "content-security-policy": "default-src 'none'; img-src 'self'; style-src 'unsafe-inline'; base-uri 'none'; form-action 'none'; frame-ancestors 'none'",
      "content-type": "text/html; charset=utf-8",
      "referrer-policy": "no-referrer",
      "x-content-type-options": "nosniff",
    },
  });
  return request.method === "HEAD" ? new Response(null, response) : response;
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
    body { display: grid; place-items: center; color: #fff; font-family: ui-monospace, SFMono-Regular, Menlo, Monaco, Consolas, monospace; }
    main { display: grid; justify-items: center; gap: 28px; width: min(90vw, 960px); }
    img { display: block; width: min(90vw, 960px); height: auto; }
    a { border: 1px solid #3a3a3a; color: #d8d8d8; font-size: 12px; letter-spacing: .12em; padding: 11px 15px; text-decoration: none; text-transform: uppercase; transition: border-color 140ms ease, color 140ms ease; }
    a:hover { border-color: #fff; color: #fff; }
    a:focus-visible { outline: 2px solid #fff; outline-offset: 4px; }
    @media (prefers-reduced-motion: reduce) { a { transition: none; } }
  </style>
</head>
<body>
  <main>
    <img src="${LOGO_PATH}" alt="Workman">
    <a href="/download">Download Workman <span aria-hidden="true">&#8594;</span></a>
  </main>
</body>
</html>`;
  return pageResponse(request, html, "public, max-age=300, must-revalidate");
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

function formattedSize(bytes: number): string {
  if (!Number.isFinite(bytes) || bytes < 0) return "Size unavailable";
  if (bytes < 1024) return `${bytes} B`;
  const units = ["KB", "MB", "GB"];
  let value = bytes / 1024;
  let unit = units[0];
  for (const candidate of units.slice(1)) {
    if (value < 1024) break;
    value /= 1024;
    unit = candidate;
  }
  const precision = value >= 10 ? 0 : 1;
  return `${value.toFixed(precision)} ${unit}`;
}

function formattedDate(value: string): string {
  const date = new Date(value);
  if (Number.isNaN(date.valueOf())) return "Date unavailable";
  return new Intl.DateTimeFormat("en", {
    day: "numeric",
    month: "short",
    timeZone: "UTC",
    year: "numeric",
  }).format(date);
}

function releasePath(release: ReleaseManifest, asset: ReleaseAsset): string {
  return `/versions/${encodeURIComponent(release.version)}/${encodeURIComponent(asset.name)}`;
}

function renderDownloadAsset(
  release: ReleaseManifest,
  asset: ReleaseAsset,
  label: string,
  format: string,
): string {
  return `<a class="asset" href="${releasePath(release, asset)}">
    <span class="asset-copy"><strong>${escapeHtml(label)}</strong><small>${escapeHtml(asset.name)}</small></span>
    <span class="asset-meta"><span>${escapeHtml(format)}</span><span>${formattedSize(asset.size)}</span></span>
  </a>`;
}

function renderDownloadGroup(release: ReleaseManifest, group: typeof DOWNLOAD_GROUPS[number]): string {
  const assets = group.assets.flatMap((definition) => {
    const asset = release.assets.find((candidate) => candidate.target === definition.target);
    return asset === undefined
      ? []
      : [renderDownloadAsset(release, asset, definition.label, definition.format)];
  });
  if (assets.length === 0) return "";
  return `<section class="platform" aria-labelledby="${group.id}">
    <div class="platform-heading">
      <p>${group.platform}</p>
      <h2 id="${group.id}">${group.architecture}</h2>
      <span>${group.description}</span>
    </div>
    <div class="assets">${assets.join("")}</div>
  </section>`;
}

async function serveDownload(request: Request, env: WorkerEnv): Promise<Response> {
  try {
    const release = await readChannel(env, "stable");
    const checksums = release.assets.find((asset) => asset.target === "checksums" || asset.name === "SHA256SUMS");
    const macArchive = release.assets.find((asset) => asset.target === "macos-arm64");
    const linuxAppImage = release.assets.find((asset) => asset.target === "linux-x86_64-appimage")
      ?? release.assets.find((asset) => asset.target === "linux-arm64-appimage");
    const linuxDeb = release.assets.find((asset) => asset.target === "linux-x86_64-deb")
      ?? release.assets.find((asset) => asset.target === "linux-arm64-deb");
    const linuxArchive = release.assets.find((asset) => asset.target === "linux-x86_64")
      ?? release.assets.find((asset) => asset.target === "linux-arm64");
    const published = formattedDate(release.published_at);
    const groups = DOWNLOAD_GROUPS.map((group) => renderDownloadGroup(release, group)).join("");
    const checksumLink = checksums === undefined
      ? ""
      : `<a class="checksum" href="${releasePath(release, checksums)}">SHA256SUMS <span>${formattedSize(checksums.size)}</span></a>`;
    const macName = escapeHtml(macArchive?.name ?? "the macOS ZIP");
    const appImageName = escapeHtml(linuxAppImage?.name ?? "the AppImage");
    const debName = escapeHtml(linuxDeb?.name ?? "the .deb package");
    const archiveName = escapeHtml(linuxArchive?.name ?? "the tar.gz archive");
    const version = escapeHtml(release.version);
    const publishedAt = escapeHtml(release.published_at);
    const html = `<!doctype html>
<html lang="en">
<head>
  <meta charset="utf-8">
  <meta name="viewport" content="width=device-width, initial-scale=1">
  <title>Download Workman ${version}</title>
  <style>
    :root { color-scheme: dark; --black: #000; --ink: #f6f6f3; --muted: #969696; --line: #2b2b2b; --panel: #0b0b0b; --hover: #151515; }
    * { box-sizing: border-box; }
    html { background: var(--black); }
    body { margin: 0; background: var(--black); color: var(--ink); font-family: Inter, ui-sans-serif, -apple-system, BlinkMacSystemFont, "Segoe UI", sans-serif; }
    a { color: inherit; }
    a:focus-visible { outline: 2px solid #fff; outline-offset: 4px; }
    .shell { width: min(1120px, calc(100% - 40px)); margin: 0 auto; padding: 46px 0 88px; }
    header { display: flex; align-items: center; justify-content: space-between; gap: 24px; padding-bottom: 42px; border-bottom: 1px solid var(--line); }
    .logo { display: block; width: min(310px, 62vw); height: auto; }
    .back { color: var(--muted); font: 11px/1 ui-monospace, SFMono-Regular, Menlo, monospace; letter-spacing: .12em; text-decoration: none; text-transform: uppercase; }
    .back:hover { color: var(--ink); }
    .release { display: grid; grid-template-columns: minmax(0, 1fr) auto; align-items: end; gap: 32px; padding: 64px 0 46px; }
    .eyebrow, .platform-heading p { margin: 0 0 11px; color: var(--muted); font: 11px/1 ui-monospace, SFMono-Regular, Menlo, monospace; letter-spacing: .15em; text-transform: uppercase; }
    h1 { margin: 0; font-size: clamp(42px, 8vw, 92px); font-weight: 620; letter-spacing: -.065em; line-height: .9; }
    h1 span { color: #777; font-weight: 420; }
    .release-meta { display: grid; gap: 8px; justify-items: end; color: var(--muted); font: 12px/1.4 ui-monospace, SFMono-Regular, Menlo, monospace; text-align: right; }
    .release-meta strong { color: var(--ink); font-weight: 500; }
    .checksum { display: flex; gap: 14px; color: var(--ink); text-underline-offset: 4px; }
    .checksum span { color: var(--muted); }
    .notice { display: flex; align-items: baseline; gap: 12px; margin: 0 0 22px; padding: 15px 17px; border: 1px solid var(--line); color: #c9c9c9; font-size: 13px; line-height: 1.55; }
    .notice b { flex: 0 0 auto; color: var(--ink); font: 10px/1 ui-monospace, SFMono-Regular, Menlo, monospace; letter-spacing: .14em; text-transform: uppercase; }
    .platform { display: grid; grid-template-columns: minmax(210px, .7fr) minmax(0, 1.7fr); gap: 44px; padding: 36px 0; border-top: 1px solid var(--line); }
    .platform:last-of-type { border-bottom: 1px solid var(--line); }
    .platform-heading h2 { margin: 0; font-size: clamp(25px, 4vw, 38px); font-weight: 520; letter-spacing: -.035em; }
    .platform-heading span { display: block; max-width: 260px; margin-top: 10px; color: var(--muted); font-size: 13px; line-height: 1.5; }
    .assets { display: grid; gap: 8px; }
    .asset { display: flex; align-items: center; justify-content: space-between; gap: 24px; min-height: 76px; padding: 16px 18px; border: 1px solid var(--line); background: var(--panel); text-decoration: none; transition: background 140ms ease, border-color 140ms ease, transform 140ms ease; }
    .asset:hover { border-color: #606060; background: var(--hover); transform: translateX(3px); }
    .asset-copy { display: grid; min-width: 0; gap: 5px; }
    .asset-copy strong { font-size: 15px; font-weight: 520; }
    .asset-copy small { overflow: hidden; color: var(--muted); font: 10px/1.35 ui-monospace, SFMono-Regular, Menlo, monospace; text-overflow: ellipsis; white-space: nowrap; }
    .asset-meta { display: grid; flex: 0 0 auto; gap: 5px; color: var(--muted); font: 10px/1.25 ui-monospace, SFMono-Regular, Menlo, monospace; letter-spacing: .07em; text-align: right; }
    .asset-meta span:first-child { color: var(--ink); }
    .install { display: grid; grid-template-columns: .7fr 1.7fr; gap: 44px; padding-top: 76px; }
    .install h2 { margin: 0; font-size: 28px; font-weight: 520; letter-spacing: -.035em; }
    .steps { display: grid; gap: 28px; }
    .steps section { display: grid; gap: 9px; }
    .steps h3 { margin: 0; font-size: 14px; font-weight: 560; }
    .steps p { margin: 0; color: var(--muted); font-size: 13px; line-height: 1.65; }
    code { color: #ddd; font: 11px/1.55 ui-monospace, SFMono-Regular, Menlo, monospace; overflow-wrap: anywhere; }
    pre { margin: 2px 0 0; padding: 14px 16px; border-left: 2px solid #ddd; background: var(--panel); white-space: pre-wrap; }
    @media (max-width: 700px) {
      .shell { width: min(100% - 28px, 1120px); padding-top: 28px; }
      header { padding-bottom: 28px; }
      .release { grid-template-columns: 1fr; padding-top: 48px; }
      .release-meta { justify-items: start; text-align: left; }
      .notice { align-items: flex-start; flex-direction: column; }
      .platform, .install { grid-template-columns: 1fr; gap: 24px; }
      .install { padding-top: 56px; }
    }
    @media (max-width: 440px) {
      .asset { align-items: flex-start; flex-direction: column; gap: 11px; }
      .asset-meta { grid-auto-flow: column; gap: 12px; text-align: left; }
    }
    @media (prefers-reduced-motion: reduce) { .asset { transition: none; } }
  </style>
</head>
<body>
  <div class="shell">
    <header>
      <a href="/" aria-label="Workman home"><img class="logo" src="${LOGO_PATH}" alt="Workman"></a>
      <a class="back" href="/">Home</a>
    </header>
    <main>
      <section class="release" aria-labelledby="release-title">
        <div>
          <p class="eyebrow">Current stable</p>
          <h1 id="release-title">Workman <span>v${version}</span></h1>
        </div>
        <div class="release-meta">
          <span>Published <strong><time datetime="${publishedAt}">${published}</time></strong></span>
          ${checksumLink}
        </div>
      </section>
      <p class="notice"><b>Access</b><span>Downloads require the access password. Your browser will ask once; use any username and enter the key as the password.</span></p>
      <div class="downloads">${groups}</div>
      <section class="install" aria-labelledby="install-title">
        <h2 id="install-title">Install notes</h2>
        <div class="steps">
          <section>
            <h3>macOS</h3>
            <p>Unzip <code>${macName}</code>, then drag <strong>Workman.app</strong> into Applications.</p>
          </section>
          <section>
            <h3>CLI + daemon</h3>
            <p>Install or update <code>wrk</code> and <code>workmand</code> from the stable channel:</p>
            <pre><code>curl -fsSL https://workman.userdefined.io/install.sh | WORKMAN_KEY='your-password' sh</code></pre>
          </section>
          <section>
            <h3>Linux</h3>
            <p>For <code>${appImageName}</code>, run <code>chmod +x &lt;file&gt;</code>. Install <code>${debName}</code> with <code>sudo apt install ./&lt;file&gt;</code>, or unpack <code>${archiveName}</code> with <code>tar -xzf &lt;file&gt;</code>.</p>
          </section>
        </div>
      </section>
    </main>
  </div>
</body>
</html>`;
    return pageResponse(request, html);
  } catch (cause) {
    console.error(JSON.stringify({
      event: "download_page_unavailable",
      message: cause instanceof Error ? cause.message : String(cause),
    }));
    return errorResponse(503, "download page is temporarily unavailable");
  }
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
    if (pathname === "/download") return serveDownload(request, env);
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
