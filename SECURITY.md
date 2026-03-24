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

- **Pack content writes** — pack files are fetched inline from the registry and written to a local store, then applied to CLI config directories. weave rejects absolute paths and path traversal (`..`) components before writing any file.
- **CLI config file writes** — adapters write to `~/.claude/`, `~/.gemini/`, and similar directories. Writes are tracked in a sidecar manifest and must be idempotent.
- **Registry fetches** — weave fetches pack metadata and releases over HTTPS from a configured registry URL.

## Out of scope

- Social engineering attacks where a user knowingly installs a pack they were explicitly warned about (e.g. dismissed hook consent prompts)
- Issues in third-party MCP servers referenced by packs (report to the server's maintainer)
- Prompt injection attacks that do not bypass weave's own security boundaries
