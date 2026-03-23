# Registry Protocol

This document is the authoritative specification for the PackWeave registry protocol. It is intended for contributors, alternative registry operators, and AI assistants working in this codebase.

---

## Overview

The pack registry is a GitHub-hosted repository (`PackWeave/registry`) that serves pack metadata and file content. It is separate from MCP server registries (like the official MCP Registry or Smithery) — weave packs are composable bundles of MCP server configuration, system prompts, slash commands, and settings, not individual MCP server listings.

The registry uses a two-tier sparse index so clients never download more than they need. Pack content is embedded directly in `packs/{name}.json` as a flat map of relative path → file content — no tarballs, no release artifacts, no SHA256 ceremony.

---

## Repository Structure

```
PackWeave/registry/
├── index.json              Lightweight search catalog
├── packs/
│   └── {name}.json         Full metadata + inline file content per pack
├── src/
│   └── {name}/
│       ├── pack.toml       Canonical pack source — reviewed by maintainers
│       ├── prompts/
│       ├── commands/
│       ├── skills/
│       └── settings/
├── TEMPLATE/               Starter template for contributors
│   └── pack.toml
├── README.md
└── CONTRIBUTING.md
```

A GitHub Actions workflow automatically regenerates `packs/{name}.json` and `index.json` from `src/` on every merge to main. Contributors only ever touch files under `src/`.

---

## Sparse Index Protocol

### Tier 1 — `index.json` (lightweight catalog)

Fetched once for `weave search` and `weave list`. Contains only what is needed to display results — no version arrays, no file content.

**URL:** `{registry_base_url}/index.json`

**Format:** A flat JSON object mapping pack names to their listing.

```json
{
  "filesystem": {
    "name": "filesystem",
    "description": "Read and write local files via the MCP filesystem server",
    "keywords": ["filesystem", "files", "mcp"],
    "latest_version": "0.1.0"
  },
  "github": {
    "name": "github",
    "description": "GitHub repos, issues, pull requests, and code search via MCP",
    "keywords": ["github", "git", "mcp"],
    "latest_version": "0.1.0"
  }
}
```

The client fetches this file once and caches it in-process for the lifetime of the command. It is never written to disk.

### Tier 2 — `packs/{name}.json` (per-pack metadata + content)

Fetched on demand when installing or resolving a specific pack. Contains all versions and their complete file content inline.

**URL:** `{registry_base_url}/packs/{name}.json`

**Format:** A `PackMetadata` object.

```json
{
  "name": "filesystem",
  "description": "Read and write local files via the MCP filesystem server",
  "authors": ["PackWeave"],
  "license": "MIT",
  "repository": "https://github.com/PackWeave/registry",
  "keywords": ["filesystem", "files", "mcp"],
  "versions": [
    {
      "version": "0.1.0",
      "dependencies": {},
      "files": {
        "pack.toml": "[pack]\nname = \"filesystem\"\n...",
        "prompts/system.md": "# System prompt content...",
        "commands/review.md": "# Review command..."
      }
    }
  ]
}
```

`files` is a flat map of relative path → file content. The store writes each entry directly to `~/.packweave/packs/{name}/{version}/` — no tarball download, no SHA256 verification step. Trust is provided by TLS and GitHub as the content host.

The client caches this per-pack after the first fetch for the lifetime of the command.

### Data Flow — `weave install`

```
weave install filesystem
        │
        ├─ resolve token (WEAVE_TOKEN env → credentials file → None)
        │
        ├─ GET {base}/packs/filesystem.json
        │   ├─ Authorization: Bearer <token>  (if authenticated)
        │   └─ {versions: [{version, files, dependencies}]}
        │
        ├─ resolve: pick version satisfying constraints
        │
        ├─ write files from release.files to ~/.packweave/packs/filesystem/0.1.0/
        │   (path-validated: no .., no absolute paths)
        │
        └─ apply to installed CLIs
```

### Data Flow — `weave search`

```
weave search filesystem
        │
        ├─ resolve token (WEAVE_TOKEN env → credentials file → None)
        │
        ├─ GET {base}/index.json  [cached after first call]
        │   ├─ Authorization: Bearer <token>  (if authenticated)
        │   └─ {filesystem: {description, latest_version}, ...}
        │
        ├─ filter by "filesystem" (name, description, keywords)
        │
        └─ print results
```

---

## Configuration

The client reads `registry_url` from `~/.packweave/config.toml`. This is the **base URL** — the client appends `/index.json` and `/packs/{name}.json` paths itself.

The `WEAVE_REGISTRY_URL` environment variable overrides `registry_url` (used by E2E tests).

Default: `https://raw.githubusercontent.com/PackWeave/registry/main`

---

## Authentication

### When is authentication needed?

| Use case | Auth required? | Why |
|----------|---------------|-----|
| `weave install`, `search`, `update` | No (recommended) | The registry is public, but authenticated requests get 5,000 req/hr instead of 60 |
| `weave publish` | **Yes** | Pushes pack content to the registry repository via the GitHub API |
| Community taps | N/A | Tokens are never sent to community taps — only to the official registry's trusted hosts |
| Private/self-hosted registries | Not currently supported | Tokens are only sent to hosts on the trusted allowlist (GitHub hosts). Alternative registry hosts do not receive tokens |

### Token lifecycle

```mermaid
sequenceDiagram
    participant User
    participant CLI as weave auth login
    participant GH as GitHub API
    participant FS as ~/.packweave/credentials

    User->>CLI: weave auth login --token ghp_xxxxx
    CLI->>CLI: Validate token format
    alt Invalid format
        CLI-->>User: ✗ Token does not match expected format
    end
    CLI->>GH: GET /user (Bearer ghp_xxxxx)
    alt Token valid
        GH-->>CLI: 200 {login: "username"}
        CLI-->>User: ✓ Authenticated as username
    else Token invalid or network error
        GH-->>CLI: 401 / timeout
        CLI-->>User: ⚠ Could not verify (stored anyway)
    end
    CLI->>FS: Write token (chmod 600)
```

**Step by step:**

1. **User creates a GitHub PAT** at [github.com/settings/tokens](https://github.com/settings/tokens)
   - For read-only operations (install, search, update): any valid token works — no special scopes needed
   - For publishing (not yet implemented): will require write access to the registry repo, limited to maintainers/collaborators
2. **`weave auth login`** — prompts for the token on stdin, validates against GitHub API (best-effort), writes to `~/.packweave/credentials`
3. **All subsequent commands** automatically include `Authorization: Bearer` in requests to trusted GitHub hosts
4. **`weave auth logout`** — deletes the credentials file

### Token resolution and request flow

```mermaid
flowchart TD
    A[weave install / search / update] --> B{WEAVE_TOKEN env var set?}
    B -->|Yes| D[Use env token]
    B -->|No| C{~/.packweave/credentials exists?}
    C -->|Yes| E[Read file token]
    C -->|No| F[No token — anonymous]
    D --> T{Is target host in trusted allowlist?}
    E --> T
    T -->|Yes| G[GET with Authorization: Bearer header]
    T -->|No| H2[GET without auth — token withheld]
    F --> H[GET without auth — 60 req/hr limit]
    G --> I[5,000 req/hr rate limit]
    H2 --> H
```

### Token resolution order

When weave needs a token, it checks these sources in order:

1. **`WEAVE_TOKEN` environment variable** — highest priority, never written to disk. Use this in CI/automation.
2. **Credentials file** — a plain-text file containing a single token, restricted to owner-only permissions (0o600) on Unix. By default this is `~/.packweave/credentials` (written by `weave auth login`). If `auth_token_path` is set in `config.toml`, that path is used instead. The override path must be under `~/.packweave/` — paths outside this directory are rejected.

The credentials file must be a regular file, not a symlink. Symlinks are rejected for security reasons, and the error message may not explain why — if you see an unexpected read failure, check for symlinks.

If neither source provides a token, requests are sent without authentication (anonymous).

### Trusted host allowlist

Tokens are **not** sent to every host. The registry client maintains a trusted host allowlist that currently includes only GitHub hosts (`api.github.com` and `raw.githubusercontent.com`). The `Authorization: Bearer <token>` header is attached only to requests whose target host matches this allowlist.

This means:
- **The default GitHub-backed registry** receives the token (it is served from `raw.githubusercontent.com`).
- **Alternative registries on non-GitHub hosts** do not receive the token, even if one is configured. Alternative registry operators cannot currently rely on weave sending Bearer tokens for access control.
- **Community taps** never receive the token — tap fetches always pass `None` for authentication.

The `validate_github_token` check on login is GitHub-specific and advisory — it does not prevent storing tokens intended for future use with other registries.

### Security properties

The authentication system enforces the following security properties:

1. **Tokens only sent to trusted GitHub hosts.** The `Authorization: Bearer` header is attached only when the request target matches the trusted host allowlist (`api.github.com`, `raw.githubusercontent.com`). All other hosts receive unauthenticated requests.
2. **Tokens never sent to community taps.** Tap fetches always pass `None` for authentication, regardless of whether a token is configured.
3. **Symlink rejection.** The credentials file must be a regular file. Symlinks are rejected to prevent redirection attacks.
4. **Restricted file permissions.** On Unix, the credentials file is written with mode `0o600` (owner read/write only).
5. **`auth_token_path` constrained to `~/.packweave/`.** Custom credential file paths cannot escape the packweave directory, preventing reads of arbitrary files.
6. **Token format validation.** Tokens are validated for printable ASCII characters only — control characters, newlines, and non-ASCII bytes are rejected to prevent HTTP header injection.
7. **Environment variable override for CI.** `WEAVE_TOKEN` allows CI/automation to authenticate without writing tokens to disk.

---

## Pack Format — `pack.toml`

Every pack must contain a `pack.toml` at its root. The canonical format uses a `[pack]` section header for pack metadata, with `[[servers]]`, `[targets]`, `[dependencies]`, and `[extensions.*]` as top-level TOML sections.

```toml
[pack]
# Required
name = "my-pack"              # Unique identifier: lowercase letters, digits, hyphens only
version = "0.1.0"             # Semver
description = "..."           # One sentence

# Recommended
authors = ["Name <email>"]
license = "MIT"
repository = "https://github.com/..."
keywords = ["keyword1", "keyword2"]

# Optional
min_tool_version = "0.4.0"   # Minimum weave version required to use this pack

# MCP servers (zero or more [[servers]] blocks)
[[servers]]
name = "server-name"         # Must be globally unique across all installed packs
command = "npx"              # Executable for stdio transport
args = ["-y", "@org/pkg@1.2.3"]
transport = "stdio"          # "stdio" (default) or "http"

# For HTTP/SSE transport, use url instead of command/args:
# url = "https://your-server.example.com/mcp"
# headers = { Authorization = "Bearer ${API_KEY}" }

# Optional: expose only a subset of the server's tools
tools = ["tool1", "tool2"]

# Declare required environment variables (never store values here)
[servers.env.API_KEY]
required = true
secret = true
description = "Your API key — get one at https://example.com"

# Target CLIs (omit this section entirely to target all supported CLIs)
[targets]
claude_code = true
gemini_cli = true
codex_cli = true

# CLI-specific settings merged non-destructively into the CLI's settings file
[extensions.claude_code]
# Any valid Claude Code settings JSON fragment
```

---

## Pack File Layout

The `files` map in `packs/{name}.json` mirrors the source layout under `src/{name}/`. All paths are relative; no leading `/`; no `..` components.

```
pack.toml                        Required — the manifest
prompts/claude.md                Optional — appended to Claude Code's CLAUDE.md
prompts/gemini.md                Optional — appended to Gemini CLI's GEMINI.md
prompts/codex.md                 Optional — appended to Codex CLI's AGENTS.md
prompts/system.md                Optional — fallback prompt when CLI-specific file is absent
commands/{name}.md               Optional — slash commands for Claude Code
skills/{name}.md                 Optional — skill files for Codex CLI
settings/claude.json             Optional — deep-merged into ~/.claude/settings.json
settings/gemini.json             Optional — deep-merged into ~/.gemini/settings.json
settings/codex.toml              Optional — merged into ~/.codex/config.toml
```

All content is plain text (TOML, JSON, Markdown) — no binaries. MCP server code lives on npm/PyPI/GitHub and is fetched at runtime by the CLI; weave only distributes the configuration that points at it.

---

## Running an Alternative Registry

Any HTTP server that serves the two endpoints below is a valid registry:

| Endpoint | Content |
|----------|---------|
| `GET /index.json` | Lightweight catalog: `HashMap<name, {name, description, keywords, latest_version}>` |
| `GET /packs/{name}.json` | Full metadata with inline `files` content |

To point weave at an alternative registry:

```toml
# ~/.packweave/config.toml
registry_url = "https://your-registry.example.com"
```

Or for a single command:

```sh
WEAVE_REGISTRY_URL=https://your-registry.example.com weave search mypack
```

---

## JSON Schemas

### `index.json`

```json
{
  "<pack-name>": {
    "name": "string",
    "description": "string",
    "keywords": ["string"],
    "latest_version": "semver string"
  }
}
```

### `packs/{name}.json`

```json
{
  "name": "string",
  "description": "string",
  "authors": ["string"],
  "license": "string | null",
  "repository": "string | null",
  "keywords": ["string"],
  "versions": [
    {
      "version": "semver string",
      "dependencies": {
        "<pack-name>": "semver requirement string"
      },
      "files": {
        "<relative-path>": "file content string"
      }
    }
  ]
}
```

All fields with `null` or array defaults may be omitted from JSON responses; the client uses `#[serde(default)]`.
