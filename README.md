# Weave

[![Build](https://github.com/PackWeave/weave/actions/workflows/ci.yml/badge.svg)](https://github.com/PackWeave/weave/actions/workflows/ci.yml)
[![Homebrew](https://img.shields.io/badge/homebrew-PackWeave%2Ftap-FBB040)](https://github.com/PackWeave/homebrew-tap)
[![Registry](https://img.shields.io/badge/registry-browse%20packs-8B5CF6)](https://github.com/PackWeave/registry)
![Platform](https://img.shields.io/badge/platform-macOS%20%7C%20Linux-lightgrey)
[![License](https://img.shields.io/badge/license-Apache%202.0-blue.svg)](LICENSE)

> **One command configures all your AI CLIs.** Install, share, and version MCP servers, slash commands, and prompts across Claude Code, Gemini CLI, and Codex CLI.

```bash
weave install web-dev    # Puppeteer MCP → Claude Code, Gemini CLI, Codex CLI
weave install git-tools  # Git MCP server — add alongside, no conflicts
weave remove web-dev     # clean undo — your manual edits stay untouched
```

---

## 🔥 The problem

Every AI CLI has its own configuration format. Setting up MCP servers, slash commands, and system prompts means hand-editing JSON and Markdown files scattered across different directories with different schemas.

There's no way to share your setup with a teammate, version it, or switch between contexts (work vs. personal vs. open source). Every developer starts from scratch.

**Weave fixes this.**

---

## ⚡ See it in action

**One command, all CLIs configured:**

```bash
weave install web-dev
# → Puppeteer MCP server
#   configured in Claude Code, Gemini CLI, and Codex CLI
```

**Stack packs — dependencies resolve automatically:**

```bash
weave install web-dev
weave install postgres
# → PostgreSQL MCP added alongside Puppeteer, no conflicts
```

**Switch contexts instantly:**

```bash
weave profile create work && weave profile add github -p work
weave profile create oss && weave profile add web-dev -p oss

weave use work    # → all CLIs reconfigured for GitHub tools
weave use oss     # → switch to web development setup
```

**Discover MCP servers from the official registry:**

```bash
weave search --mcp filesystem
# MCP Registry results for 'filesystem':
#   Filesystem Server
#     Package:    @modelcontextprotocol/server-filesystem (npm)
#     Repository: https://github.com/modelcontextprotocol/servers
```

**Pin packs to a project — teammates get the same setup:**

```bash
weave install web-dev --project
# → MCP servers written to both ~/.claude.json (user)
#   and .mcp.json in this repo (project scope)

weave list
# web-dev v1.0.0 — Web development MCP stack
#   Scope: user + project (/Users/dev/my-app)
```

**Safe and reversible — your manual config stays untouched:**

```bash
weave diagnose      # detect config drift across all CLIs
weave sync          # fix it — reapply your profile
weave remove web-dev # clean undo, manual edits survive
```

**Create and share your own packs:**

```bash
weave init my-pack
# → scaffolds pack.toml, prompts/, commands/, skills/, settings/
# edit, test, publish — anyone can `weave install my-pack`
```

---

## ⚙️ How it works

Think of packs like Homebrew formulas for your AI CLI setup — community-maintained, versioned, one-line install.

A **pack** is a `pack.toml` manifest bundled with MCP server definitions, slash commands, system prompt fragments, and settings. Packs install into the active **profile** — a named set of packs for a specific context (`work`, `oss`, `personal`). Create profiles with `weave profile create`, switch with `weave use`, and recover from config drift with `weave sync`.

```
weave install web-dev
        │
        ├─▶ fetches pack content from the registry
        ├─▶ resolves transitive dependencies
        └─▶ applies to each installed CLI — non-destructively

        Claude Code:  ~/.claude.json, ~/.claude/settings.json, ~/.claude/commands/,
                      ~/.claude/CLAUDE.md
                      + .mcp.json (with --project)
        Gemini CLI:   ~/.gemini/settings.json, ~/.gemini/GEMINI.md
        Codex CLI:    ~/.codex/config.toml, ~/.codex/AGENTS.md, ~/.codex/skills/
```

Each CLI has its own **adapter** — a thin layer that knows exactly how to read and write that CLI's config format. Adapters never wipe your existing config. They only add, track, and cleanly remove what they own. A `weave remove` is surgical.

---

## 📥 Installation & quickstart

**Homebrew (macOS and Linux):**

```bash
brew install PackWeave/tap/weave
```

**cargo-binstall (installs a pre-built binary, no compiler needed):**

```bash
cargo binstall packweave
```

**Shell script (macOS and Linux):**

```bash
curl -fsSL https://raw.githubusercontent.com/PackWeave/weave/main/install.sh | sh
```

**Build from source:**

```bash
cargo install --git https://github.com/PackWeave/weave
```

> [!NOTE]
> Weave targets macOS and Linux. Windows is not officially supported or tested in CI.

**Try your first pack:**

```bash
weave install web-dev       # installs Puppeteer MCP across all your AI CLIs
weave list                  # see what's installed
weave remove web-dev        # clean undo
```

---

## 🔧 Commands

| Command | Description |
|---------|-------------|
| `weave install <pack>` | Install a pack and apply it to all supported CLIs. Use `--version` to pin (e.g. `^1.0`, `=1.2.0`). Use `--project` to also write to `.mcp.json` in the current directory. |
| `weave remove <pack>` | Remove a pack and clean up all config entries it wrote |
| `weave list` | Show installed packs with versions, scope, and target CLIs |
| `weave search <query>` | Search the pack registry |
| `weave search --mcp <query>` | Search the official MCP Registry for servers |
| `weave update [pack]` | Update one or all packs to the latest compatible version |
| `weave init [name]` | Scaffold a new pack directory |
| `weave diagnose [--json]` | Check for config drift and health issues across all CLIs |
| `weave profile create <name>` | Create a new named profile |
| `weave profile list` | List all profiles (marks the active one) |
| `weave profile delete <name>` | Delete a profile |
| `weave profile add <pack> -p <name>` | Add a pack to a named profile |
| `weave use [profile]` | Switch to a named profile, or print the active one |
| `weave sync` | Reapply the active profile to all adapters |

---

## 📦 Pack format

A pack is a directory with a `pack.toml` manifest at its root:

```toml
[pack]
name = "web-dev"
version = "1.0.0"
description = "Web development MCP stack"
authors = ["yourname"]
keywords = ["web", "browser", "git"]

[[servers]]
name = "puppeteer"
command = "npx"
args = ["-y", "@modelcontextprotocol/server-puppeteer"]
transport = "stdio"

[[servers]]
name = "filesystem"
command = "npx"
args = ["-y", "@modelcontextprotocol/server-filesystem"]
transport = "stdio"
```

Packs can also declare:

- **Dependencies** on other packs — resolved transitively
- **Slash commands / skills** — copied into `~/.claude/commands/` or `~/.codex/skills/`
- **System prompt fragments** — appended to `CLAUDE.md` / `GEMINI.md` / `AGENTS.md` between tagged delimiters
- **Settings fragments** — deep-merged into Claude Code and Gemini CLI JSON settings; merged as top-level keys in Codex CLI's TOML config
- **Environment variable declarations** — written as `${VAR}` references, never values

> [!IMPORTANT]
> Packs never store secret values. Env vars are written as `${MY_API_KEY}` references into CLI config files — the actual values come from your shell environment at runtime.

> [!TIP]
> Test your pack locally before publishing: `weave install ./my-pack` — idempotent, re-reads files on each run.

See [pack.schema.toml](https://github.com/PackWeave/weave/blob/main/pack.schema.toml) for the full annotated schema and [docs/PACKS.md](https://github.com/PackWeave/weave/blob/main/docs/PACKS.md) for quality guidelines.

---

## 🖥️ Supported CLIs

| CLI | Status | What Weave manages |
|-----|--------|--------------------|
| **Claude Code** | ✅ Supported | MCP servers · slash commands · system prompt · settings |
| **Gemini CLI** | ✅ Supported | MCP servers · system prompt · settings |
| **Codex CLI** | ✅ Supported | MCP servers · skills · system prompt · settings |

---

## 🗂️ Project-scope config

Some CLIs read both a user-level config (`~/.claude/`) and a project-level config (`.mcp.json` in your repo). By default, `weave install` only writes to user scope. Pass `--project` to also write MCP servers to `.mcp.json` in the current directory:

```bash
weave install web-dev --project
```

> [!TIP]
> `weave remove` cleans up both user and project scope automatically, regardless of which directory you run it from.

Run `weave diagnose` to detect this condition automatically:

```
Profile: default
Packs: 1 installed

  web-dev v1.0.0
    Claude Code: drifted (server 'puppeteer' (from pack 'web-dev') is tracked but missing from claude.json)
    Gemini CLI: ok
    Codex CLI: ok

1 issue(s) found. Run `weave sync` to fix.
```

---

## 🚀 Coming next

See [docs/ROADMAP.md](https://github.com/PackWeave/weave/blob/main/docs/ROADMAP.md) for full milestones.

- **Hooks** — pack-defined lifecycle hooks with explicit opt-in (`--allow-hooks`)
- **Community taps** — `weave tap add user/repo` for third-party pack sources
- **Remote MCP servers** — `url`/`headers` transport in all adapters
- **Org config sharing** — team-wide profiles and pack sets

---

## 📚 Docs

| Document | Description |
|----------|-------------|
| [docs/ARCHITECTURE.md](https://github.com/PackWeave/weave/blob/main/docs/ARCHITECTURE.md) | Internal design: modules, data flow, adapter contracts |
| [docs/ROADMAP.md](https://github.com/PackWeave/weave/blob/main/docs/ROADMAP.md) | Milestones and planned scope |
| [docs/CONTRIBUTING.md](https://github.com/PackWeave/weave/blob/main/docs/CONTRIBUTING.md) | How to contribute |
| [docs/PACKS.md](https://github.com/PackWeave/weave/blob/main/docs/PACKS.md) | Pack format and quality guidelines |

AI assistants working in this repo should read [`AGENTS.md`](https://github.com/PackWeave/weave/blob/main/AGENTS.md).

---

## 🤝 Contributing

See [docs/CONTRIBUTING.md](https://github.com/PackWeave/weave/blob/main/docs/CONTRIBUTING.md).

---

## 📄 License

Apache 2.0 — Copyright 2026 Brenno Rangel Ferrari. See [LICENSE](./LICENSE).
