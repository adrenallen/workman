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
install.sh
```

`GET /releases.json` combines the two channel pointers into the public manifest. Versioned
artifact responses use a one-year immutable cache policy. The channel manifest is cached for one
minute, and the current installer for five minutes. The Worker streams artifact bodies directly
from R2 and supports byte-range downloads.

## First deploy

Wrangler must report the User Defined Cloudflare account before any mutation:

```sh
npm ci
npx wrangler whoami
npx wrangler r2 bucket create workman-releases
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
archive, uploads the same checksum-verified files to R2, refreshes `install.sh`, and moves the
`latest` pointer. After validation, `scripts/promote.sh vX.Y.Z` moves the R2 `stable` pointer and
promotes the corresponding GitHub release.

An existing local artifact set can be republished without creating a tag or GitHub release:

```sh
npm run publish -- release \
  --version 0.1.1 \
  --artifacts-dir ../../release/v0.1.1 \
  --published-at 2026-08-05T22:24:43Z \
  --notes-url https://github.com/adrenallen/workman/releases/tag/v0.1.1 \
  --installer ../../scripts/release-assets/install.sh
npm run promote -- --version 0.1.1
```

`scripts/generate-manifest.mjs` verifies every artifact against `SHA256SUMS` before any upload.
Both publication commands are idempotent and use Wrangler's authenticated remote R2 operations.
