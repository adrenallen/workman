# Contributing to Workman

Thank you for helping improve Workman. Open an issue before a large behavioral or architectural
change so the approach can be agreed before implementation.

## Development workflow

1. Fork the repository and create a focused branch from `main`.
2. Keep generated files, build output, local Workman state, and credentials out of commits.
3. Run the relevant checks before opening a pull request:

   ```sh
   cargo fmt --all -- --check
   cargo test --workspace --locked
   (cd apps/desktop && npm ci && npm run check && npm run build)
   (cd infra/update-host && npm ci && npm test && npm run check)
   ```

4. Explain user-visible behavior, security implications, and validation in the pull request.

Small documentation-only changes do not need the complete native application build. Platform or
release changes should include the narrowest automated regression test that proves the behavior.

## Security and privacy

Never commit real tokens, API keys, signing material, `.env` files, Workman runtime data, terminal
output, or customer/user information. The repository runs Gitleaks on pull requests, but automated
detection is a backstop rather than permission to submit sensitive data. Follow
[SECURITY.md](SECURITY.md) for private vulnerability reports.

Pull requests from forks run with read-only repository permissions and receive no release,
Cloudflare, Apple, or signing credentials. Release and deployment workflows must remain separate
from untrusted pull-request code.
