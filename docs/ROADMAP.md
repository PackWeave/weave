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
- [x] AGENTS.md (canonical AI instructions; CLAUDE.md, GEMINI.md, CODEX.md are thin pointers)
- [x] pack.schema.toml
- [x] GitHub issue templates (bug, feature, pack)

-----

## Milestone 2 — MVP core (v0.1) ✅

> First usable release: install/list/remove packs for Claude Code + Gemini CLI, backed by a GitHub registry.

- [x] `cargo init` with correct crate name and metadata
- [x] Core commands: `install`, `list`, `remove`
- [x] Pack manifest parsing + validation (TOML)
- [x] Local store: extract and cache packs
- [x] Lock file for pinned versions
- [x] GitHub-backed registry index (read-only)
- [x] Seed registry with 10–15 starter packs (13 packs shipped — see issue #21)
- [x] Claude Code adapter (servers, prompts, commands, settings)
- [x] Gemini CLI adapter (servers, prompts, settings)
- [x] One-line install script
- [x] CI: build + clippy + fmt check on push

-----

## Milestone 3 — v0.2

> Pack authoring, MCP Registry search, and distribution improvements. Codex CLI adapter shipped. All code complete — crates.io publishing pending infra setup (issue #78).

- [x] Codex CLI adapter (servers, skills, prompts, settings)
- [x] `weave search --mcp` against the official MCP Registry (pack registry search shipped in M2)
- [x] `weave update` for pack version management
- [x] `weave init` — scaffold a new pack
- [x] Environment variable handling for secrets (write `${VAR}` references only)
- [x] Recursive transitive dependency resolution with cycle detection
- [x] Improved conflict detection using declared tool lists
- [ ] Publish `packweave` crate to crates.io (requires `CARGO_REGISTRY_TOKEN` secret — see issue #78)
- [x] SHA256 checksums alongside release binaries (see issue #40)
- [x] ARM Linux release target — `aarch64-unknown-linux-gnu` via `cross` (see issue #41)

-----

## Milestone 4 — v0.3

> Hooks, profiles, and community taps. Profiles, diagnose expansion, and sync are shipped — hooks and taps remain.

- [ ] Hooks support via `extensions.<cli>.hooks` with explicit opt-in
- [x] Profiles: group packs into named sets
- [x] `weave use <profile>`
- [ ] Community taps (`weave tap add user/repo`)
- [x] `weave diagnose` — full config drift and health check across all adapters (per-pack, per-adapter status with `--json` output)
- [x] `weave sync` — reapply active profile
- [ ] Support remote MCP servers (`url`/`headers`) in all CLI adapters (see issue #59)

-----

## Milestone 5 — v0.4+

> Quality-of-life and ecosystem features.

- [ ] Template management for CLI prompt files (CLAUDE.md, AGENTS.md, etc.)
- [ ] Auto-update mechanism
- [ ] Plugin system for extending weave
- [ ] Org/team configuration sharing
- [ ] Release Please integration — automated CHANGELOG.md and release PRs via `develop` → `main` merge flow

-----

## Explicitly deferred (no planned milestone)

- **GUI or TUI** — weave is a CLI tool only
- **Windows support** — weave targets macOS and Linux; Windows is not tested in CI. The Claude Code adapter works on Windows as a best-effort target, but full Windows support is not planned
- **MCP server execution or sandboxing** — weave configures; it does not run
- **IDE plugins** — out of scope
- **Paid registry tiers** — registry is free and open for now
- **Private registries** — the registry trait supports it architecturally, but no UX for it yet
- **Pack signing / verified publishers** — important eventually, not v1
