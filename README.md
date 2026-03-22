# Weave

[![Build](https://github.com/PackWeave/weave/actions/workflows/ci.yml/badge.svg)](https://github.com/PackWeave/weave/actions/workflows/ci.yml)
![Status](https://img.shields.io/badge/status-milestone%203%20complete-green)
[![Homebrew](https://img.shields.io/badge/homebrew-PackWeave%2Ftap-FBB040)](https://github.com/PackWeave/homebrew-tap)
[![Registry](https://img.shields.io/badge/registry-browse%20packs-8B5CF6)](https://github.com/PackWeave/registry)
![Platform](https://img.shields.io/badge/platform-macOS%20%7C%20Linux-lightgrey)
[![License](https://img.shields.io/badge/license-Apache%202.0-blue.svg)](LICENSE)

> **A pack manager for AI CLIs** — install, share, and version MCP servers, slash commands, and prompts across Claude Code, Gemini CLI, and Codex CLI.

```bash
weave install @webdev    # install a web dev MCP stack
weave list               # see what's installed and where
weave diagnose           # verify config health across all your CLIs
```

---

## The problem

Every AI CLI has its own configuration format. Setting up MCP servers, slash commands, and system prompts means hand-editing JSON and Markdown files scattered across different directories with different schemas.

There's no way to share your setup with a teammate, version it, or switch between contexts (work vs. personal vs. open source). Every developer starts from scratch.

**Weave fixes this.**

---

## ⚙️ How it works

Think of packs like Homebrew formulas for your AI CLI setup — community-maintained, versioned, one-line install.

A **pack** is a `pack.toml` manifest bundled with MCP server definitions, slash commands, system prompt fragments, and settings. Packs install into the active **profile** — a named set of packs for a specific context (`work`, `oss`, `personal`). Create named profiles with `weave profile create work`, switch with `weave use work`, and recover from config drift with `weave sync`.

```
weave install @webdev
        │
        ├─▶ fetches + verifies the pack archive from the registry
        ├─▶ resolves transitive dependencies
        └─▶ applies to each installed CLI — non-destructively

        Claude Code:  ~/.claude.json, ~/.claude/settings.json, ~/.claude/commands/,
                      ~/.claude/CLAUDE.md, ~/.claude/.packweave_manifest.json
        Gemini CLI:   ~/.gemini/settings.json, ~/.gemini/GEMINI.md,
                      ~/.gemini/.packweave_manifest.json
        Codex CLI:    ~/.codex/config.toml, ~/.codex/AGENTS.md,
                      ~/.codex/skills/, ~/.codex/.packweave_manifest.json
        (+ project-scope equivalents when ./.claude/, ./.gemini/, or ./.codex/ exist,
           plus Claude Code's ./.mcp.json for project-scope MCP servers)
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

Override the install directory by setting `WEAVE_INSTALL_DIR` (default: `/usr/local/bin`, fallback: `~/.local/bin`):

```bash
WEAVE_INSTALL_DIR=~/.local/bin curl -fsSL https://raw.githubusercontent.com/PackWeave/weave/main/install.sh | sh
```

**Build from source:**

```bash
# Compile and install directly from the git repo (requires Rust)
cargo install --git https://github.com/PackWeave/weave

# Or clone and build manually
git clone https://github.com/PackWeave/weave
cd weave
cargo build --release
```

> [!NOTE]
> Weave targets macOS and Linux. Windows is not officially supported or tested in CI.

---

## ⚡ Quick start

```bash
# 1. Install a pack from the registry
weave install @webdev

# 2. See what was installed and which CLIs it applied to
weave list

# 3. Confirm everything looks healthy
weave diagnose
```

That's it. Weave has written the pack's MCP servers, system prompt, settings, and (for Claude Code) slash commands into your CLI config — tracking everything it wrote so `weave remove` can undo it cleanly.

---

## 🔧 Commands

| Command | Description |
|---------|-------------|
| `weave install <pack>` | Download a pack, resolve its dependencies, and apply it to all supported CLIs. Use `-v`/`--version` to pin a version requirement (e.g. `^1.0`, `=1.2.0`). |
| `weave remove <pack>` | Remove a pack and clean up all config entries it wrote |
| `weave list` | Show installed packs, their versions, and which CLIs they were applied to |
| `weave search <query>` | Search the registry for packs matching a keyword or phrase |
| `weave search --mcp <query>` | Search the official MCP Registry for servers instead of weave packs |
| `weave update [pack]` | Update one or all installed packs to the latest compatible version |
| `weave init [name]` | Scaffold a new pack directory (omit name to initialize the current directory) |
| `weave diagnose [--json]` | Check for config drift and health issues across all installed CLIs and packs |
| `weave profile create <name>` | Create a new named profile |
| `weave profile list` | List all profiles (marks the active one) |
| `weave profile delete <name>` | Delete a profile (refuses if active or default) |
| `weave profile add <pack> -p <name>` | Add a pack reference to a named profile |
| `weave use [profile]` | Switch to a named profile, or print the active profile |
| `weave sync` | Reapply the active profile's lock file to all adapters |

**Examples:**

```bash
# Install a pack (@ prefix resolves from the registry)
weave install @webdev

# Pin a specific version requirement
weave install @webdev --version "=1.2.0"

# Remove a pack
weave remove webdev

# Search the registry
weave search "browser automation"

# Search the MCP Registry for servers
weave search --mcp "filesystem"

# Update all installed packs
weave update

# Update a specific pack
weave update webdev

# Scaffold a new pack
weave init my-pack

# Check for config issues (e.g. project-scope directories added after install)
weave diagnose

# JSON output for scripting
weave diagnose --json

# Create and switch to a named profile
weave profile create work
weave profile add @webdev -p work
weave use work

# Reapply the active profile after manual config changes
weave sync
```

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

- 📦 **Dependencies** on other packs — Weave resolves them transitively
- 💬 **Slash commands / skills** — copied into `~/.claude/commands/` or `~/.codex/skills/` with namespaced filenames
- 📝 **System prompt fragments** — appended to `CLAUDE.md` / `GEMINI.md` / `AGENTS.md` between tagged delimiters
- ⚙️ **Settings fragments** — merged into each CLI's settings: Claude/Gemini JSON settings are deep-merged; Codex settings are added as top-level keys in `config.toml`
- 🔐 **Environment variable declarations** — written as references, never values

> [!IMPORTANT]
> Packs never store secret values. Env vars are written as `${MY_API_KEY}` references into CLI config files — the actual values come from your shell environment at runtime. Pack authors must not embed credentials or tokens in a pack.

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
> If you create a `.claude/`, `.gemini/`, or `.codex/` directory _after_ installing a pack, run `weave install <pack>` again from the project directory. `apply` is idempotent — it's safe to re-run and will add any missing project-scope config without duplicating anything.

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
