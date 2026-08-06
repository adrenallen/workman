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
immutable cache policy. `/install.sh` is also public, but the bootstrap requires a key when it
fetches `/releases.json` and `/versions/*`. Versioned responses keep their one-year immutable cache
policy and byte-range support.

## Download keys

The `DOWNLOAD_KEYS` Worker secret is a comma-separated list. Production currently uses one key
compiled into the app updater and a second key that can be shared with friends. A protected request
is authorized by any one of these forms:

```text
Authorization: Bearer <key>
X-Workman-Key: <key>
?key=<key>
Authorization: Basic <base64(any-username:key)>
```

Browser navigations without a key receive `401` and `WWW-Authenticate: Basic realm="workman"`;
the username may be anything and the password is the shared key. API requests receive a JSON 401.
Keep the two keys in the Cloudflare secret rather than `wrangler.jsonc` or git:

```sh
printf '%s' '<app-key>,<friends-key>' | npx wrangler secret put DOWNLOAD_KEYS
```

The app updater sends its key as a Bearer token on both the manifest and artifact request. The
other accepted forms are for manual testing and friend downloads.

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
archive, uploads the same checksum-verified files to R2, refreshes the keyed bootstrap at
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

## Friend flow

To download in a browser, open a concrete artifact URL such as
`https://workman.userdefined.io/versions/0.1.1/workman-macos-arm64.zip`. At the browser prompt, use
any username and the friends key as the password.

To install the stable bundle from a terminal, pass the same key without putting it in the URL:

```sh
curl -fsSL https://workman.userdefined.io/install.sh | \
  sh -s -- --key '<friends-key>'

# Equivalent environment-variable form:
curl -fsSL https://workman.userdefined.io/install.sh | \
  WORKMAN_KEY='<friends-key>' sh
```

Before a prerelease is promoted, install it from the latest channel explicitly:

```sh
curl -fsSL https://workman.userdefined.io/install.sh | \
  sh -s -- --key '<friends-key>' --channel latest
```

The bootstrap sends the key as a Bearer token to both `/releases.json` and the selected artifact,
checks the manifest SHA-256 before extracting, and then runs the bundle-local installer.
