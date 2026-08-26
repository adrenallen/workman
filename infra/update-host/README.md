# Workman update host

This Worker serves Workman update metadata and immutable release artifacts from the
`workman-releases` R2 bucket at `https://workman.userdefined.io`.

## Storage contract

```text
versions/<version>/<release asset>
versions/<version>/SHA256SUMS
_manifests/<version>.json
channels/latest.json
channels/stable.json
branding/workman-logo-wide-transparent.png
install.sh
```

`GET /` is a public black lander whose centered logo is served from the R2 branding object with an
immutable cache policy. `/download`, `/install.sh`, `/releases.json`, and `/versions/*` are public.
Versioned responses keep their one-year immutable cache policy and byte-range support.

Search indexing is temporarily disabled by the single `SITE_NOINDEX` variable in `wrangler.jsonc`.
While it is `"true"`, every response carries `X-Robots-Tag: noindex, nofollow`, HTML pages include a
robots meta tag, and `/robots.txt` disallows the entire site. Change that one value to `"false"` to
remove all three blocks.

## Public download contract

Release metadata and artifacts are intentionally anonymous because the canonical GitHub Releases
are public. Older Workman clients still send their former shared Bearer key; the Worker ignores
that header, so already-installed versions continue to update during the transition. New clients
send no credential by default. `--key`, `WORKMAN_UPDATE_KEY`, and `config.yml`'s `update.key` remain
available only for private, self-hosted update services.

Do not add another shared key to source or a shipped binary. Anything embedded in a distributed
client must be treated as public and cannot enforce download authorization.

## First deploy

Wrangler must report the User Defined Cloudflare account before any mutation:

```sh
npm ci
npx wrangler whoami
npx wrangler r2 bucket create workman-releases
npx wrangler r2 object put \
  workman-releases/branding/workman-logo-wide-transparent.png \
  --remote \
  --file ../../assets/branding/workman-logo-wide-transparent.png \
  --content-type image/png \
  --cache-control 'public, max-age=31536000, immutable'
npx wrangler r2 object put workman-releases/install.sh \
  --remote \
  --file install.sh \
  --content-type 'text/x-shellscript; charset=utf-8' \
  --cache-control 'public, max-age=300, must-revalidate'
npm run types
npm test
npm run check
npm run deploy
```

The custom-domain entry in `wrangler.jsonc` creates the DNS record and certificate for
`workman.userdefined.io` in the existing `userdefined.io` Cloudflare zone. If `whoami` does not
show the expected account, or deploy cannot access that zone, stop instead of changing the route.

## Publish and promote

Normal publishing is driven by `scripts/release.sh`. It retains GitHub Releases as the durable
archive, uploads the same checksum-verified files to R2, refreshes the public bootstrap at
`infra/update-host/install.sh`, and moves the `latest` pointer. After validation,
`scripts/promote.sh vX.Y.Z` moves the R2 `stable` pointer and promotes the corresponding GitHub
release.

An existing local artifact set can be republished without creating a tag or GitHub release:

```sh
npm run publish -- release \
  --version 0.1.1 \
  --artifacts-dir ../../release/v0.1.1 \
  --published-at 2026-08-05T22:24:43Z \
  --notes-url https://github.com/adrenallen/workman/releases/tag/v0.1.1 \
  --installer ./install.sh
npm run promote -- --version 0.1.1
```

`scripts/generate-manifest.mjs` verifies every artifact against `SHA256SUMS` before any upload.
Both publication commands are idempotent and use Wrangler's authenticated remote R2 operations.

## Release retention

R2 is a delivery cache; GitHub Releases remain the durable archive. Retention is channel-aware:

- always keep every version referenced by `channels/stable.json` or `channels/latest.json`;
- keep the greatest version below the current stable as the one rollback release;
- delete every older `versions/<version>/*` object and its `_manifests/<version>.json`;
- leave channel, installer, branding, and unrecognized namespaces untouched.

Do not configure an age-based R2 lifecycle rule for release objects. Time alone cannot identify the
stable/latest targets or the prior stable, so such a rule can delete a live download or its rollback.

The prune command uses the dedicated `wrangler.prune.jsonc` remote binding. It recomputes the full
policy immediately before deleting each version so a concurrent publish, promotion, or rollback
fails closed. Dry-run is the default and itemizes every object and byte:

```sh
npm run prune
npm run prune -- --dry-run
npm run prune -- --yes  # permanent production deletion
```

Successful release publication and promotion invoke `--yes` automatically. Pruning is deliberately
fail-safe there: a failure emits a loud warning but cannot fail or roll back the release operation.
Cloudflare currently prices Standard R2 storage at $0.015/GB-month with an account-wide 10
GB-month free tier; see https://developers.cloudflare.com/r2/pricing/ for current pricing.

## Public install flow

To download in a browser, open `https://workman.userdefined.io/download`. The page and its stable
release links are anonymous.

To install the stable bundle from a terminal:

```sh
curl -fsSL https://workman.userdefined.io/install.sh | \
  sh

# Skip the interactive replacement/restart confirmations:
curl -fsSL https://workman.userdefined.io/install.sh | \
  sh -s -- --yes
```

Before a prerelease is promoted, install it from the latest channel explicitly:

```sh
curl -fsSL https://workman.userdefined.io/install.sh | \
  sh -s -- --channel latest
```

The bootstrap prints the selected channel and the exact version that channel currently serves and
checks the manifest SHA-256 before extracting. It inventories deduplicated `wrk`, `workmand`, and obsolete
pre-Workman launchers from PATH and known install locations. Versioned bundle directories remain
available as rollback files; superseded launchers are backed up before being replaced.

`WORKMAN_INSTALL_DIR` overrides only the extracted versioned bundle destination (including the app,
guides, and manual installer). The bundled installer keeps CLI binaries in the durable
`$HOME/.local/share/workman/dist/<version>/bin` layout and links them from `$HOME/.local/bin`; the
bootstrap's reconciliation and final PATH verification use that durable target regardless of the
bundle destination override.

When old launchers or daemons are present, an interactive install reads confirmation from
`/dev/tty`, so prompting works under `curl | sh`. `--yes` skips the prompts; a non-interactive
install also proceeds. A confirmed daemon restart preserves its existing data directory. At the
end, the bootstrap performs a fresh PATH walk, verifies that the selected `wrk` is the newly
installed version, and fails with the offending path when another binary still wins. It also
prints a `hash -r` reminder for shells that cached the old command location.

On macOS, the bootstrap also offers to copy the bundled `Workman.app` to `/Applications` so the
app is available through Launchpad and Spotlight. Updates refresh that copy. An existing app is
replaced only when its `CFBundleIdentifier` is `com.workman.desktop`; a different bundle is left
untouched and the install fails with an explanation. `--yes` and non-interactive installs accept
the app-copy step as well. When present, the `/Applications` copy is also the first bundle chosen
by `wrk app`.
