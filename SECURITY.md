# Security policy

## Supported versions

Security fixes are made on `main` and shipped in the newest Workman release. Please reproduce a
report against the latest release or `main` when practical.

## Reporting a vulnerability

Do not open a public issue for an unpatched vulnerability or an exposed credential. Use GitHub's
[private vulnerability reporting](https://github.com/adrenallen/workman/security/advisories/new)
instead. Include the affected version, impact, reproduction steps, and any proposed mitigation.
Do not include real credentials or unrelated user data in the report.

If a credential is accidentally committed, revoke or rotate it immediately before attempting a
history rewrite. Removing a file or commit does not invalidate a credential, and public forks or
clones may retain the original object.

## Local trust model

Workman is a local development orchestrator. It launches shells, developer commands, and AI coding
agents with the permissions configured by the user. Several bundled agent presets intentionally use
their vendors' unattended or broad-permission modes. Review every command and preset before running
it against valuable files or credentials, and use operating-system isolation for untrusted code.

The daemon control and MCP endpoints bind to loopback and use credentials stored in the Workman data
directory. Do not publish that directory, `mcp-endpoint.json`, exported process environments, agent
output, or local configuration. Repository security checks cannot detect secrets written only to a
user's runtime data.

## Release integrity

Installers accept release metadata only from the configured host, constrain artifact URLs to that
origin and version path, and verify the manifest SHA-256 before installation. Maintainers should
follow [scripts/RELEASING.md](scripts/RELEASING.md) and never expose signing or deployment
credentials to pull-request workflows.
