# Roadmap

This document describes what weave will build, in what order, and what is explicitly deferred.

The milestones below are sequential. Each one produces something usable before the next begins.

-----

## Milestone 1 — Repo foundation ✅

> The repo is set up with clear documentation, architecture design, and contribution guidelines. No functional code yet.

- [x] README
- [x] docs/ARCHITECTURE.md
- [x] docs/ROADMAP.md
- [x] docs/CONTRIBUTING.md
- [x] CLAUDE.md
- [x] GEMINI.md
- [x] CODEX.md
- [x] pack.schema.toml
- [x] GitHub issue templates (bug, feature, pack)

-----

## Milestone 2 — MVP core (v0.1)

> First usable release: install/list/remove packs for Claude Code + Gemini CLI, backed by a GitHub registry.

- [ ] `cargo init` with correct crate name and metadata
- [ ] Core commands: `install`, `list`, `remove`
- [ ] Pack manifest parsing + validation (TOML)
- [ ] Local store: extract and cache packs
- [ ] Lock file for pinned versions
- [ ] GitHub-backed registry index (read-only)
- [ ] Seed registry with 10–15 starter packs
- [ ] Claude Code adapter (servers, prompts, commands, settings)
- [ ] Gemini CLI adapter (servers, prompts, settings)
- [ ] One-line install script
- [ ] Homebrew formula
- [ ] CI: build + clippy + fmt check on push

-----

## Milestone 3 — v0.2

> Codex support, official registry search, and pack creation workflow.

- [ ] Codex CLI adapter (servers, prompts, settings)
- [ ] `weave search` against the official MCP Registry
- [ ] `weave update` for pack version management
- [ ] `weave init` — scaffold a new pack
- [ ] Environment variable handling for secrets (write references only)
- [ ] Improved conflict detection using declared tool lists

-----

## Milestone 4 — v0.3

> Hooks, profiles, and community taps.

- [ ] Hooks support via `extensions.<cli>.hooks` with explicit opt-in
- [ ] Profiles: group packs into named sets
- [ ] `weave use <profile>`
- [ ] Community taps (`weave tap add user/repo`)
- [ ] `weave doctor` — detect config drift across CLIs
- [ ] `weave sync` — reapply active profile

-----

## Milestone 5 — v0.4+

> Quality-of-life and ecosystem features.

- [ ] Template management for CLI prompt files (CLAUDE.md, AGENTS.md, etc.)
- [ ] Auto-update mechanism
- [ ] Plugin system for extending weave
- [ ] Org/team configuration sharing

-----

## Explicitly deferred (no planned milestone)

- **GUI or TUI** — weave is a CLI tool only
- **Windows support** — macOS and Linux first
- **MCP server execution or sandboxing** — weave configures; it does not run
- **IDE plugins** — out of scope
- **Paid registry tiers** — registry is free and open for now
- **Private registries** — the registry trait supports it architecturally, but no UX for it yet
- **Pack signing / verified publishers** — important eventually, not v1
