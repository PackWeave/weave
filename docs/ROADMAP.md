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
- [x] Prepare `packweave` crate for crates.io publishing (workflow ready; actual publish deferred to M5 first release)
- [x] SHA256 checksums alongside release binaries (see issue #40)
- [x] ARM Linux release target — `aarch64-unknown-linux-gnu` via `cross` (see issue #41)

-----

## Milestone 4 — v0.3

> Hooks, profiles, community taps, and remote MCP servers. All features shipped.

- [x] Hooks support via `extensions.<cli>.hooks` with explicit opt-in
- [x] Profiles: group packs into named sets
- [x] `weave use <profile>`
- [x] Community taps (`weave tap add user/repo`)
- [x] `weave diagnose` — full config drift and health check across all adapters (per-pack, per-adapter status with `--json` output)
- [x] `weave sync` — reapply active profile
- [x] Support remote MCP servers (`url`/`headers`) in all CLI adapters (see issue #59)

-----

## Milestone 5 — v0.4 (first public release) ✅

> Pack publishing, error quality, and release infrastructure. Ship a polished, complete product that developers can install, use, author for, and trust.

### Ecosystem

- [x] `weave publish` command for pack authors (issue #146)
- [x] `weave auth` for registry authentication (issue #147)

### Correctness & Security

- [x] Additive hook merge for multi-pack coexistence (issue #145)
- [x] Normalize local paths before hashing in store cache key (issue #133)
- [x] Validate MCP server header values for plaintext secrets (issue #141)

### Library Quality

- [x] Replace `anyhow` with `WeaveError` in CLI/core handlers — required for crates.io consumers (issue #143)
- [x] Colorize CLI output (issue #106)

### Release Infrastructure

- [x] Release Please integration — automated CHANGELOG.md and release PRs (issue #43)
- [x] Set up `CARGO_REGISTRY_TOKEN` for crates.io publishing (issue #78)
- [x] Manual E2E validation on macOS, including all Claude Code hook events supported by the spec (issue #93)
- [x] Cut first public release — GitHub Release with binaries, Homebrew, cargo-binstall (issue #92)

-----

## Milestone 6 — v0.5 (security hardening)

> Harden security boundaries and fix correctness gaps before growing the ecosystem. These items were elevated from the original M6 scope because they affect trust and safety for config files that control AI CLI behavior.

### Security & Correctness

- [ ] `--dry-run` flag on install/sync/remove — preview changes without writing (issue #166)
- [ ] Concurrency lock to prevent simultaneous weave operations (issue #201)
- [ ] Pack content checksums in registry for integrity verification (issue #175)
- [ ] Enforce `min_tool_version` check during pack install (issue #197)
- [ ] Switch Codex adapter to `toml_edit` to preserve user comments (issue #212)

### Adoption Accelerators

- [ ] `weave export` — reverse-engineer an existing CLI setup into a publishable pack (issue #162)
- [ ] Passive update check for installed packs (issue #202)

-----

## Milestone 7 — v0.6 (ecosystem depth)

> Richer authoring tools, adapter modernization, and onboarding improvements. Ships incrementally after security hardening.

### Onboarding & Authoring

- [ ] Template placeholder substitution — `${PROJECT_NAME}`, `${PACK_NAME}` in prompt fragments (issue #165)
- [ ] `[prerequisites]` section in pack.toml — declare system dependency checks with actionable hints (issue #161)
- [ ] `weave diff` — show config changes since last install (issue #213)
- [ ] `weave doctor` — verify MCP servers are reachable and functional (issue #214)

### Adapter Modernization

- [ ] Migrate Claude Code commands to new skills format — `~/.claude/skills/` (issue #61)
- [ ] Support Claude Code rules directory — `.claude/rules/` (issue #62)
- [ ] Write project-scope GEMINI.md and CLAUDE.md for prompt packs (issue #60)
- [ ] Skill directory distribution — multi-file skills with reference materials (issue #170)

### Ecosystem Power Features

- [ ] Template management for CLI prompt files — CLAUDE.md, AGENTS.md, etc. (issue #50)
- [ ] Pack-defined health checks in `weave diagnose` (issue #163)
- [ ] `weave config get/set` command (issue #200)
- [ ] Subagent distribution via packs (issue #198)

### Quality & Testing

- [ ] Golden-file tests for adapter config output (issue #216)
- [ ] Fuzz testing targets for config parsing (issue #217)
- [ ] Windows build-check CI job (issue #218)
- [ ] Use FNV-1a for local pack cache directory hashing (issue #132)
- [ ] Include source info in `Store::list_cached` return type (issue #134)
- [ ] Cover `CompositeRegistry` directly instead of mock reimplementation (issue #142)
- [ ] Decouple `core::use_profile` from `GitHubRegistry` (issue #144)

-----

## Milestone 8 — v0.7 (power features)

> Higher-risk features that expand the attack surface. Each requires careful security design before implementation.

- [ ] Post-install scripts — `[scripts]` table in pack.toml (issue #167). Requires explicit `--allow-scripts` flag, sandboxing design, and restricted action set. See security analysis for design constraints.
- [ ] Auto-update mechanism (issue #51). Must be opt-in per pack, show diff of changes, require confirmation. Passive update check (#202) should ship first.
- [ ] Registry namespace scoping — `@scope/pack-name` format (issue #215). Protocol-level change; design before the registry has many consumers.

-----

## Explicitly deferred (no planned milestone)

- **Plugin system** — no concrete use case; revisit if demand emerges
- **Org/team configuration sharing** — profiles + taps already cover team workflows
- **GUI or TUI** — weave is a CLI tool only
- **Windows support** — weave targets macOS and Linux; Windows is not tested in CI. The Claude Code adapter works on Windows as a best-effort target, but full Windows support is not planned
- **MCP server execution or sandboxing** — weave configures; it does not run
- **IDE plugins** — out of scope
- **Paid registry tiers** — registry is free and open for now
- **Private registries** — the registry trait supports it architecturally, but no UX for it yet
- **Pack signing / verified publishers** — important eventually, not v1
