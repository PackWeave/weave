# Weave

> MCP pack manager for AI CLIs.

Weave is a command-line tool that installs, manages, and publishes **packs** — versioned bundles of MCP server configurations, slash commands, system prompts, and tool settings — across multiple AI CLIs.

Think **oh-my-zsh for AI CLIs**: portable, shareable packs that configure Claude Code, Gemini CLI, and Codex CLI with a single command.

```bash
weave install @webdev
weave list
weave diagnose
```

---

## The problem

Every AI CLI (Claude Code, Gemini CLI, Codex CLI) has its own configuration format. Setting up MCP servers, slash commands, and system prompts means hand-editing JSON and Markdown files in different locations, with different schemas. There's no way to share a setup with your team, version it, or switch between contexts.

Weave fixes this.

---

## How it works

A **pack** is a `pack.toml` manifest plus a set of files — MCP server definitions, commands, prompts, settings fragments. You install packs into a **profile** (e.g. `work`, `oss`, `personal`). When you switch profiles, Weave rewrites the config files of every installed AI CLI to reflect the active pack set.

Weave ships with **adapters** for each supported CLI. An adapter knows exactly how to read and write that CLI's config format — non-destructively. It never wipes your existing setup; it only adds, tracks, and cleanly removes what it owns.

---

## Status

> **v0.1 (MVP)** — `install`, `list`, `remove`, `search`, and `diagnose` are functional for Claude Code and Gemini CLI. Not yet published to a package registry; install via the shell script or build from source.

---

## Docs

See [docs/README.md](./docs/README.md) for the full index, or jump straight to:

- [docs/ARCHITECTURE.md](./docs/ARCHITECTURE.md)
- [docs/ROADMAP.md](./docs/ROADMAP.md)
- [docs/CONTRIBUTING.md](./docs/CONTRIBUTING.md)

AI assistants working in this repo should read [`AGENTS.md`](./AGENTS.md).

---

## Supported CLIs

| CLI | Status |
|-----|--------|
| Claude Code | ✅ v0.1 |
| Gemini CLI | ✅ v0.1 |
| OpenAI Codex CLI | Planned (v0.2) |

---

## Installation

**Homebrew (macOS and Linux):**

```bash
brew install PackWeave/tap/weave
```

**Shell script (macOS and Linux):**

```bash
curl -fsSL https://raw.githubusercontent.com/PackWeave/weave/main/install.sh | sh
```

Set `WEAVE_INSTALL_DIR` to override the install location (default: `/usr/local/bin`, fallback: `~/.local/bin`):

```bash
WEAVE_INSTALL_DIR=~/.local/bin curl -fsSL https://raw.githubusercontent.com/PackWeave/weave/main/install.sh | sh
```

**Build from source:**

```bash
git clone https://github.com/PackWeave/weave
cd weave
cargo build --release
```

---

## Usage

```bash
# Install a pack from the registry
weave install @webdev

# List installed packs
weave list

# Remove a pack
weave remove webdev

# Search the registry
weave search "browser automation"

# Check CLI configuration health
weave diagnose
```

---

## Coming soon

These features are actively planned. See [docs/ROADMAP.md](./docs/ROADMAP.md) for milestones and details.

**v0.2 — Codex, profiles, and pack authoring:**

```bash
# Switch between named profiles (e.g. work, oss, personal)
weave use work

# Create a new pack scaffold
weave init my-pack

# Update installed packs to latest compatible versions
weave update

# Publish a pack to the registry
weave publish my-pack
```

**v0.3 — Hooks, taps, and sync:**

```bash
# Add a community pack source
weave tap add user/repo

# Reapply the active profile (after manual config changes)
weave sync
```

---

## Project-scope config and reinstall behavior

Some AI CLIs support both user-scope and project-scope configuration. For example:

- **Claude Code** reads `.claude/settings.json` and `.mcp.json` when a `.claude/` directory exists in the current project.
- **Gemini CLI** reads `.gemini/settings.json` when a `.gemini/` directory exists in the current project.

When you run `weave install`, Weave applies pack config to every scope that exists **at install time**. If you create a project-scope directory (`.claude/` or `.gemini/`) **after** installing a pack, Weave will not automatically back-fill project-scope config. The pack's MCP servers, prompts, and settings will only be present in the user-scope config until you reinstall.

**How to fix it:** Re-run `weave install <pack-name>` from the project directory. Weave's `apply` is idempotent — re-running it updates user-scope config and adds any missing project-scope config.

**How to detect it:** Run `weave diagnose` to check for this condition across all installed packs and adapters:

```bash
weave diagnose
```

Example output when a pack needs reinstalling for project scope:

```
Running diagnostics (profile 'default')...

  Claude Code — 1 issue(s) found:
    [warning] pack 'webdev' has no project-scope entries for Claude Code but .claude/ now exists — pack was installed before this directory was created
             run `weave install webdev` to apply project-scope config

  Gemini CLI — OK

1 issue(s) found. See suggestions above to fix them.
```

---

## Pack format

A pack is a directory with a `pack.toml` manifest:

```toml
[pack]
name = "webdev"
version = "1.0.0"
description = "Web development MCP stack"
authors = ["yourname"]
keywords = ["web", "browser", "git"]

[[servers]]
name = "puppeteer"
command = "npx"
args = ["-y", "@modelcontextprotocol/server-puppeteer"]
transport = "stdio"
namespace = "browser"

[[servers]]
name = "filesystem"
command = "npx"
args = ["-y", "@modelcontextprotocol/server-filesystem"]
transport = "stdio"
namespace = "fs"
```

See [pack.schema.toml](./pack.schema.toml) for the full annotated schema.

---

## Contributing

See [docs/CONTRIBUTING.md](./docs/CONTRIBUTING.md).

---

## License

Apache 2.0 — Copyright 2026 Brenno Rangel Ferrari. See [LICENSE](./LICENSE).
