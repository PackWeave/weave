# PackWeave

> MCP pack manager for AI CLIs.

PackWeave is a command-line tool that installs, manages, and publishes **packs** — versioned bundles of MCP server configurations, slash commands, system prompts, and tool settings — across multiple AI CLIs.

```bash
weave install @webdev
weave use work
weave publish my-pack
```

---

## The problem

Every AI CLI (Claude Code, Gemini CLI, Codex CLI) has its own configuration format. Setting up MCP servers, slash commands, and system prompts means hand-editing JSON and Markdown files in different locations, with different schemas. There's no way to share a setup with your team, version it, or switch between contexts.

PackWeave fixes this.

---

## How it works

A **pack** is a `pack.toml` manifest plus a set of files — MCP server definitions, commands, prompts, settings fragments. You install packs into a **profile** (e.g. `work`, `oss`, `personal`). When you switch profiles, PackWeave rewrites the config files of every installed AI CLI to reflect the active pack set.

PackWeave ships with **adapters** for each supported CLI. An adapter knows exactly how to read and write that CLI's config format — non-destructively. It never wipes your existing setup; it only adds, tracks, and cleanly removes what it owns.

---

## Status

> **Pre-release.** Not yet functional. See [ROADMAP.md](./ROADMAP.md) for what's planned.

---

## Supported CLIs

| CLI | Status |
|-----|--------|
| Claude Code | Planned (v1) |
| Gemini CLI | Planned (v2) |
| OpenAI Codex CLI | Planned (v2) |

---

## Installation

Not yet available. Will be distributed via Homebrew, `cargo install`, and a shell script.

```bash
# Coming soon
brew install packweave/tap/weave
```

---

## Usage

```bash
# Install a pack from the registry
weave install @webdev

# Install into a specific profile
weave install @rust-dev --profile work

# Switch active profile (rewrites CLI configs)
weave use work

# List installed packs
weave list

# Search the registry
weave search "browser automation"

# Publish a pack
weave publish

# Check CLI configuration health
weave doctor
```

---

## Pack format

A pack is a directory with a `pack.toml` manifest:

```toml
[pack]
name = "webdev"
version = "1.0.0"
description = "Web development MCP stack"
author = "yourname"
tags = ["web", "browser", "git"]

[[mcp.servers]]
name = "puppeteer"
command = "npx"
args = ["-y", "@modelcontextprotocol/server-puppeteer"]

[[mcp.servers]]
name = "filesystem"
command = "npx"
args = ["-y", "@modelcontextprotocol/server-filesystem"]
```

See [pack.schema.toml](./pack.schema.toml) for the full annotated schema.

---

## Contributing

See [CONTRIBUTING.md](./CONTRIBUTING.md).

---

## License

MIT
