# Roadmap

This document describes what weave will build, in what order, and what is explicitly deferred.

The milestones below are sequential. Each one produces something usable before the next begins.

-----

## Milestone 1 — Repo foundation ✅

> The repo is set up with clear documentation, architecture design, and contribution guidelines. No functional code yet.

- [x] README
- [x] ARCHITECTURE.md
- [x] ROADMAP.md
- [x] CONTRIBUTING.md
- [x] CLAUDE.md
- [x] pack.schema.toml
- [x] GitHub issue templates

-----

## Milestone 2 — Skeleton

> A compilable Rust project with all CLI commands stubbed. No real behaviour, but the full structure is in place.

- [ ] `cargo init` with correct crate name and metadata
- [ ] All `clap` commands defined (install, remove, list, use, profile, publish, search, info, update, sync, doctor, auth)
- [ ] `~/.weave/` directory initialisation on first run
- [ ] `config.toml` read/write
- [ ] Structured error types (`error.rs`)
- [ ] CI: build + clippy + fmt check on push

-----

## Milestone 3 — Pack format

> The pack manifest format is fully defined, parseable, and validated.

- [ ] `pack.toml` parsing via `serde` + `toml`
- [ ] Full validation (required fields, semver format, valid server definitions)
- [ ] `weave init` — scaffold a new `pack.toml` interactively
- [ ] `pack.schema.toml` kept in sync with implementation
- [ ] Unit tests for valid and invalid manifests

-----

## Milestone 4 — Local pack management

> Install and manage packs from a local path. No registry yet.

- [ ] Store: extract and cache a local pack directory
- [ ] Profile: create, delete, list, switch
- [ ] `weave install <path>` — install from local directory
- [ ] `weave remove <pack>`
- [ ] `weave list`
- [ ] `weave use <profile>`
- [ ] Lock file generation with pinned versions
- [ ] Basic dependency resolution (flat, no conflicts)

-----

## Milestone 5 — Claude Code adapter

> weave can fully configure Claude Code from an installed pack.

- [ ] Read and write `~/.claude/settings.json` (MCP servers)
- [ ] Copy commands into `~/.claude/commands/` with namespaced filenames
- [ ] Append/remove system prompt blocks in `~/.claude/CLAUDE.md`
- [ ] Deep-merge settings fragments
- [ ] Ownership manifest (`~/.claude/.packweave_manifest.json`)
- [ ] Clean removal — verified to leave no orphaned entries
- [ ] `weave sync` — reapply active profile to Claude Code
- [ ] `weave doctor` — detect and report config drift
- [ ] Adapter integration tests with fixture config directories

-----

## Milestone 6 — Registry (read)

> Install and search packs from the public registry.

- [ ] Registry JSON index format defined
- [ ] `PackWeave/registry` GitHub repo seeded with 3–5 initial packs
- [ ] `weave search <query>`
- [ ] `weave info <pack>`
- [ ] `weave install @pack` — download from registry
- [ ] Archive download with SHA256 verification
- [ ] `weave update` — update all installed packs to latest compatible

-----

## Milestone 7 — Registry (publish)

> Developers can publish their own packs.

- [ ] `weave auth login` — GitHub OAuth device flow
- [ ] `weave publish` — pack into archive, submit PR to registry repo
- [ ] Pre-publish validation (manifest, required files, naming rules)
- [ ] Registry review process documented in CONTRIBUTING.md

-----

## Milestone 8 — Distribution

> weave can be installed by anyone in under 30 seconds.

- [ ] GitHub Actions: cross-compile binaries for macOS arm64, macOS x86_64, Linux x86_64, Linux arm64
- [ ] GitHub Releases with binary assets
- [ ] Homebrew tap (`PackWeave/homebrew-tap`)
- [ ] `cargo install weave-cli`
- [ ] Install shell script (`curl | sh`)
- [ ] Version command (`weave --version`)

-----

## Milestone 9 — Additional adapters (v2)

> weave supports Gemini CLI and Codex CLI.

- [ ] Gemini CLI adapter
- [ ] Codex CLI adapter
- [ ] Multi-adapter `weave doctor`
- [ ] Per-target opt-out in `pack.toml` (`targets.gemini_cli = false`)

-----

## Explicitly deferred (no planned milestone)

- **GUI or TUI** — weave is a CLI tool only
- **Windows support** — macOS and Linux first
- **MCP server execution or sandboxing** — weave configures; it does not run
- **IDE plugins** — out of scope
- **Paid registry tiers** — registry is free and open for now
- **Private registries** — the registry trait supports it architecturally, but no UX for it yet
- **Pack signing / verified publishers** — important eventually, not v1
