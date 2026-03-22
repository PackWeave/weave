# Weave

[![Build](https://github.com/PackWeave/weave/actions/workflows/ci.yml/badge.svg)](https://github.com/PackWeave/weave/actions/workflows/ci.yml)
[![Homebrew](https://img.shields.io/badge/homebrew-PackWeave%2Ftap-FBB040)](https://github.com/PackWeave/homebrew-tap)
[![Registry](https://img.shields.io/badge/registry-browse%20packs-8B5CF6)](https://github.com/PackWeave/registry)
![Platform](https://img.shields.io/badge/platform-macOS%20%7C%20Linux-lightgrey)
[![License](https://img.shields.io/badge/license-Apache%202.0-blue.svg)](LICENSE)

> **A pack manager for AI CLIs** — install, share, and version MCP servers, slash commands, and prompts across Claude Code, Gemini CLI, and Codex CLI.

```bash
weave install @webdev    # Puppeteer + Git MCP servers → Claude Code, Gemini CLI, Codex CLI
weave install @databases # add Postgres + Redis — dependencies auto-resolved
weave remove databases   # clean undo — your manual edits stay untouched
```

---

## The problem

Every AI CLI has its own configuration format. Setting up MCP servers, slash commands, and system prompts means hand-editing JSON and Markdown files scattered across different directories with different schemas.

There's no way to share your setup with a teammate, version it, or switch between contexts (work vs. personal vs. open source). Every developer starts from scratch.

**Weave fixes this.**

---

## ⚡ See it in action

**One command, all CLIs configured:**

```bash
weave install @webdev
# → Puppeteer, filesystem, and Git MCP servers
#   configured in Claude Code, Gemini CLI, and Codex CLI
```

**Stack packs — dependencies resolve automatically:**

```bash
weave install @webdev
weave install @databases
# → Postgres + Redis added alongside your web dev servers, no conflicts
```

**Switch contexts instantly:**

```bash
weave profile create work && weave profile add @cloud-infra -p work
weave profile create oss && weave profile add @webdev -p oss

weave use work    # → all CLIs reconfigured for cloud infrastructure
weave use oss     # → switch to open source web dev setup
```

**Discover MCP servers from the official registry:**

```bash
weave search --mcp filesystem
# MCP Registry results for 'filesystem':
#   Filesystem Server
#     Package:    @modelcontextprotocol/server-filesystem (npm)
#     Repository: https://github.com/modelcontextprotocol/servers
```

**Safe and reversible — your manual config stays untouched:**

```bash
weave diagnose      # detect config drift across all CLIs
weave sync          # fix it — reapply your profile
weave remove webdev # clean undo, manual edits survive
```

**Create and share your own packs:**

```bash
weave init my-pack
# → scaffolds pack.toml, prompts/, commands/, skills/, settings/
# edit, test, publish — anyone can `weave install @my-pack`
```

---

## ⚙️ How it works

Think of packs like Homebrew formulas for your AI CLI setup — community-maintained, versioned, one-line install.

A **pack** is a `pack.toml` manifest bundled with MCP server definitions, slash commands, system prompt fragments, and settings. Packs install into the active **profile** — a named set of packs for a specific context (`work`, `oss`, `personal`). Create profiles with `weave profile create`, switch with `weave use`, and recover from config drift with `weave sync`.

```
weave install @webdev
        │
        ├─▶ fetches + verifies the pack archive from the registry
        ├─▶ resolves transitive dependencies
        └─▶ applies to each installed CLI — non-destructively

        Claude Code:  ~/.claude.json, ~/.claude/settings.json, ~/.claude/commands/,
                      ~/.claude/CLAUDE.md, ./.mcp.json (project-scope MCP servers)
        Gemini CLI:   ~/.gemini/settings.json, ~/.gemini/GEMINI.md
        Codex CLI:    ~/.codex/config.toml, ~/.codex/AGENTS.md, ~/.codex/skills/
        (+ project-scope equivalents when ./.claude/, ./.gemini/, or ./.codex/ exist)
```

Each CLI has its own **adapter** — a thin layer that knows exactly how to read and write that CLI's config format. Adapters never wipe your existing config. They only add, track, and cleanly remove what they own. A `weave remove` is surgical.

---

## 📥 Installation

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

---

## 🔧 Commands

| Command | Description |
|---------|-------------|
| `weave install <pack>` | Install a pack and apply it to all supported CLIs. Use `--version` to pin (e.g. `^1.0`, `=1.2.0`). |
| `weave remove <pack>` | Remove a pack and clean up all config entries it wrote |
| `weave list` | Show installed packs with versions, descriptions, and target CLIs |
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

Packs can also declare:

- **Dependencies** on other packs — resolved transitively
- **Slash commands / skills** — copied into `~/.claude/commands/` or `~/.codex/skills/`
- **System prompt fragments** — appended to `CLAUDE.md` / `GEMINI.md` / `AGENTS.md` between tagged delimiters
- **Settings fragments** — deep-merged into Claude Code and Gemini CLI JSON settings; merged as top-level keys in Codex CLI's TOML config
- **Environment variable declarations** — written as `${VAR}` references, never values

> [!IMPORTANT]
> Packs never store secret values. Env vars are written as `${MY_API_KEY}` references into CLI config files — the actual values come from your shell environment at runtime.

See [pack.schema.toml](https://github.com/PackWeave/weave/blob/main/pack.schema.toml) for the full annotated schema and [docs/PACKS.md](https://github.com/PackWeave/weave/blob/main/docs/PACKS.md) for quality guidelines.

---

## 🖥️ Supported CLIs

| CLI | Status | What Weave manages |
|-----|--------|--------------------|
| **Claude Code** | ✅ Supported | MCP servers · slash commands · system prompt · settings |
| **Gemini CLI** | ✅ Supported | MCP servers · system prompt · settings |
| **Codex CLI** | ✅ Supported | MCP servers · skills · system prompt · settings |

---

## Project-scope config

Some CLIs read both a user-level config (`~/.claude/`) and a project-level config (`.claude/` in your repo). Weave applies packs to every scope that **exists at install time**.

> [!TIP]
> If you create a `.claude/`, `.gemini/`, or `.codex/` directory _after_ installing a pack, run `weave install <pack>` again from the project directory. `apply` is idempotent — it will add the missing project-scope config without duplicating anything.

Run `weave diagnose` to detect this condition automatically:

```
Profile: default
Packs: 1 installed

  webdev v1.0.0
    Claude Code: drifted (server 'puppeteer' (from pack 'webdev') is tracked but missing from claude.json)
    Gemini CLI: ok
    Codex CLI: ok

1 issue(s) found. Run `weave sync` to fix.
```

---

## 🚀 Coming next

See [docs/ROADMAP.md](https://github.com/PackWeave/weave/blob/main/docs/ROADMAP.md) for full milestones.

**Hooks and community taps:**

```bash
weave tap add user/repo  # add a community pack source
# Hooks: pack-defined lifecycle hooks with explicit opt-in (--allow-hooks)
```

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

## License

Apache 2.0 — Copyright 2026 Brenno Rangel Ferrari. See [LICENSE](./LICENSE).
