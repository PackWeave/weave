# Security Policy

## Reporting a vulnerability

Please **do not** open a public GitHub issue for security vulnerabilities.

Report privately via GitHub's [Security Advisories](https://github.com/PackWeave/weave/security/advisories/new) feature, or email the maintainers directly through the contact on the GitHub profile.

Include:
- A clear description of the vulnerability
- Steps to reproduce with a minimal example
- The impact you believe it has
- Any remediation ideas you have

You will receive a response within 72 hours. We will work with you to understand and address the issue before any public disclosure.

---

## Scope

weave is a local CLI tool. Its attack surface includes:

- **Archive extraction** — pack archives are downloaded from the registry and extracted to disk. weave validates paths and checksums before extraction.
- **CLI config file writes** — adapters write to `~/.claude/`, `~/.gemini/`, and similar directories. Writes are tracked in a sidecar manifest and must be idempotent.
- **Registry fetches** — weave fetches pack metadata and releases over HTTPS from a configured registry URL.

## Out of scope

- Vulnerabilities requiring a malicious pack to already be installed by a trusted user
- Issues in third-party MCP servers referenced by packs (report to the server's maintainer)
- Prompt injection attacks that do not bypass weave's own security boundaries
