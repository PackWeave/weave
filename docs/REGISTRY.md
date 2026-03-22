# Registry Protocol

This document is the authoritative specification for the PackWeave registry protocol. It is intended for contributors, alternative registry operators, and AI assistants working in this codebase.

---

## Overview

The pack registry is a GitHub-hosted repository (`PackWeave/registry`) that serves pack metadata and archives. It is separate from MCP server registries (like the official MCP Registry or Smithery) — weave packs are composable bundles of MCP servers, system prompts, and slash commands, not individual MCP server listings.

The registry uses a two-tier sparse index so clients never download more than they need. At hundreds of packs, a monolithic index becomes impractical; the sparse design keeps every operation fast.

---

## Repository Structure

```
PackWeave/registry/
├── index.json              Lightweight search catalog
├── packs/
│   └── {name}.json         Full metadata per pack — fetched on demand
├── src/
│   └── {name}/
│       ├── pack.toml       Canonical pack source — reviewed by maintainers
│       └── prompts/
│           └── system.md
├── TEMPLATE/               Starter template for contributors
│   └── pack.toml
├── README.md
└── CONTRIBUTING.md
```

---

## Sparse Index Protocol

### Tier 1 — `index.json` (lightweight catalog)

Fetched once for `weave search` and `weave list`. Contains only what is needed to display results — no version arrays, no download URLs.

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

### Tier 2 — `packs/{name}.json` (per-pack metadata)

Fetched on demand when installing or resolving a specific pack. Contains all versions, download URLs, and SHA256 checksums.

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
      "url": "https://github.com/PackWeave/registry/releases/download/packs-v0.1.0/filesystem-0.1.0.tar.gz",
      "sha256": "9477b9d29b1fdc92f0a7e4bdabc5d6fd4498cd9a8b5ca846ede9865e8fd3d263",
      "size_bytes": null,
      "dependencies": {}
    }
  ]
}
```

The client caches this per-pack after the first fetch for the lifetime of the command.

### Data Flow — `weave install`

```
weave install filesystem
        │
        ├─ GET {base}/packs/filesystem.json
        │   └─ {versions: [{version, url, sha256, dependencies}]}
        │
        ├─ resolve: pick version satisfying constraints
        │
        ├─ GET {url}  (e.g. filesystem-0.1.0.tar.gz)
        │   └─ archive bytes
        │
        ├─ verify sha256
        │
        ├─ extract to ~/.packweave/packs/filesystem/0.1.0/
        │
        └─ apply to installed CLIs
```

### Data Flow — `weave search`

```
weave search filesystem
        │
        ├─ GET {base}/index.json  [cached after first call]
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

## Pack Format — `pack.toml`

Every pack archive must contain a `pack.toml` at its root. The canonical format uses a `[pack]` section header for pack metadata, with `[[servers]]`, `[targets]`, `[dependencies]`, and `[extensions.*]` as top-level TOML sections.

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
args = ["-y", "@org/pkg@latest"]
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

## Pack Archive Format

Packs are distributed as `.tar.gz` archives. The layout inside the archive:

```
pack.toml                        Required — the manifest
prompts/system.md                Optional — system prompt appended to the CLI's instruction file
commands/{name}.md               Optional — slash commands (one file per command)
```

All paths are relative; no leading `/`; no `..` path components. Symlinks and hardlinks are rejected during extraction.

The SHA256 checksum in `packs/{name}.json` is computed over the raw `.tar.gz` bytes (after gzip compression). The store verifies this before extracting.

---

## Running an Alternative Registry

Any HTTP server that serves the two endpoints below is a valid registry:

| Endpoint | Content |
|----------|---------|
| `GET /index.json` | Lightweight catalog: `HashMap<name, {name, description, keywords, latest_version}>` |
| `GET /packs/{name}.json` | Full metadata: `{name, description, authors, license, repository, keywords, versions: [...]}` |

Pack archives can be hosted anywhere — their URLs are embedded in the per-pack `versions[].url` field.

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
      "url": "string",
      "sha256": "hex string",
      "size_bytes": "integer | null",
      "dependencies": {
        "<pack-name>": "semver requirement string"
      }
    }
  ]
}
```

All fields with `null` or array defaults may be omitted from JSON responses; the client uses `#[serde(default)]`.
