# Releasing Workman

Workman releases are built locally on Apple silicon with `scripts/release.sh`. The macOS app,
`wrk`, and `workmand` must all be signed with the same Developer ID Application identity, use the
hardened runtime, and pass Apple notarization before any release artifact is published.

## One-time local setup

1. Install the Developer ID Application certificate and private key in the login Keychain.
2. Put the App Store Connect API private key in a local, access-restricted path such as
   `~/.appstoreconnect/private_keys/AuthKey_<KEY_ID>.p8` and run `chmod 600` on it.
3. Copy `scripts/release.env.example` to `~/.workman-release.env`, fill in the identity, Team ID,
   absolute key path, key ID, and issuer ID, then run `chmod 600 ~/.workman-release.env`.
4. Run `scripts/release.sh --signing-test <current-version>`. This macOS-only mode signs,
   notarizes, staples, and verifies an isolated artifact set under bundle id
   `com.workman.todo417` (override with `WORKMAN_SIGNING_TEST_BUNDLE_ID`) but cannot tag or publish.

The release preflight fails before building when the certificate is absent, the key is unreadable,
or Apple rejects the API credentials. `APPLE_API_KEY_PATH` must point to the key; the script never
copies key material into the repository or release output.

## Release trust pipeline

The local pipeline builds the app with Tauri's Developer ID signing configuration and hardened
runtime. Workman is not App Sandbox-enabled: its PTY spawning and outbound localhost/network
connections do not require entitlements, so no runtime-relaxing entitlements are granted. The
standalone CLI and daemon are signed separately with secure timestamps and hardened runtime.

The complete macOS package is submitted with one blocking `notarytool --wait --timeout 2h` call.
The first submission from a new Apple Developer account can normally take 30–60 minutes or more;
leave that command running rather than polling or resubmitting the same archive. A rejected or
timed-out submission stops the release before checksums, tagging, GitHub publication, or R2
publication. An accepted ticket is stapled to `Workman.app`; the final ZIP is rebuilt and extracted again so
`codesign --verify --deep --strict`, `stapler validate`, `spctl -a -vv`, Team ID checks, and the
same archive layout consumed by the updater are verified after packaging.

## Renewal and rotation

Developer ID Application certificates are normally valid for five years. Track the installed
certificate's expiry in Keychain Access and renew it before it expires. Install the replacement
certificate, update `APPLE_SIGNING_IDENTITY` if its display name changes, and pass a signing test
before removing the old identity. Existing notarized releases remain valid after normal certificate
expiry; do not revoke an old certificate unless its private key is compromised.

To rotate the App Store Connect key, create the replacement key, store its `.p8` outside the repo,
update the three `APPLE_API_*` values in `~/.workman-release.env`, and pass a signing test. Only then
revoke the previous API key. Apple exposes a `.p8` download once, so keep the local copy backed up
in the team's approved secret store.

The `/download` page shows the legacy Gatekeeper workaround only for versions 0.1.4 and earlier.
Do not deploy that Worker change ahead of the first signed release; Garrett owns the v0.1.5
release and deployment decision.
