# Architecture

This document describes the internal design of weave. It is intended for contributors and for AI assistants working in this codebase.

-----

## Guiding principles

**1. Non-destructive mutations.** weave never overwrites user config wholesale. Every write is additive and tracked. Every removal is surgical. A user's manual edits must survive a `weave sync`.

**2. Adapters own the CLI boundary.** All knowledge of a specific CLI's config format lives in one place — its adapter. The core never reads or writes CLI config files directly.

**3. The store is the source of truth.** The local pack cache (`~/.packweave/packs/`) is the authoritative record of what's installed. CLI config files are a derived, writable view of the store.

**4. Profiles are explicit.** There is always exactly one active profile. Switching profiles is an explicit operation with a clear before/after state — not implicit drift.

**5. The registry is pluggable.** The registry client is behind a trait. The v1 implementation is GitHub-backed, but the core doesn't depend on it.

**6. Packs sit above MCP registries.** weave does not curate individual MCP servers. It consumes upstream registries and focuses on composable packs.

-----

## High-level data flow

```
weave install @webdev
       │
       ▼
  Registry client          ← fetches pack metadata + archive URL
       │
       ▼
  Resolver                 ← resolves semver, checks conflicts, builds install plan
       │
       ▼
  Store                    ← downloads, verifies SHA256, extracts to ~/.packweave/packs/
       │
       ▼
  Profile                  ← records pack as installed in active profile
       │
       ▼
  Lock file                ← pins exact resolved versions
       │
       ▼
  Adapters (1..n)          ← each adapter applies the pack to its CLI config
```

```
weave use work
       │
       ▼
  Profile::switch()        ← computes diff: packs to add, packs to remove
       │
       ├──▶ Adapters::remove(old_packs)
       └──▶ Adapters::apply(new_packs)
```

-----

## Module structure

> **This is the intended design, not a snapshot of current source.**
> Modules that are not yet implemented are still listed here — this document
> guides implementation, it does not track it. Check `src/` for the current
> state of the code.

```
src/
  main.rs                  Entry point. Builds CLI, dispatches to handlers.
  lib.rs                   Crate root; re-exports public modules.

  cli/                     Clap command definitions and handler functions.
    mod.rs
    install.rs
    list.rs
    remove.rs
    search.rs
    update.rs
    init.rs
    publish.rs
    profile.rs
    sync.rs
    diagnose.rs
    auth.rs

  core/
    pack.rs                Pack manifest: parsing, validation, the Pack struct.
    profile.rs             Profile: read/write, active profile tracking.
    lockfile.rs            Lock file: read/write, version pinning.
    resolver.rs            Dependency graph construction and semver resolution.
    store.rs               Local pack cache: download, extract, verify, evict.
    registry.rs            Registry trait + default GitHub-backed implementation.
    config.rs              Global weave config (~/.packweave/config.toml).

  adapters/
    mod.rs                 CliAdapter trait definition.
    claude_code.rs         Claude Code adapter (~/.claude/).
    gemini_cli.rs          Gemini CLI adapter (~/.gemini/).
    codex_cli.rs           Codex CLI adapter (~/.codex/).

  error.rs                 Unified error types via thiserror.
  util.rs                  Shared helpers (file ops, path resolution, etc.)
```

-----

## Core abstractions

### `Pack`

The in-memory representation of a parsed `pack.toml`. Validated on load — a `Pack` that exists is always structurally valid.

```rust
pub struct Pack {
    pub name: String,
    pub version: semver::Version,
    pub description: String,
    pub authors: Vec<String>,
    pub license: Option<String>,
    pub repository: Option<String>,
    pub keywords: Vec<String>,
    pub min_tool_version: Option<semver::Version>,
    pub servers: Vec<McpServer>,
    pub dependencies: HashMap<String, semver::VersionReq>,
    pub extensions: PackExtensions,
    pub targets: PackTargets,
}

pub struct McpServer {
    pub name: String,
    pub package_type: Option<String>,
    pub package: Option<String>,
    pub command: String,
    pub args: Vec<String>,
    pub transport: Option<Transport>,
    pub namespace: Option<String>,
    pub tools: Vec<String>,
    pub env: HashMap<String, EnvVar>,
}

pub struct PackTargets {
    pub claude_code: bool,
    pub gemini_cli: bool,
    pub codex_cli: bool,
}

pub struct EnvVar {
    pub required: bool,
    pub secret: bool,
    pub description: Option<String>,
}
```

### `ResolvedPack`

A `Pack` with its exact version pinned and its source recorded. This is what adapters receive — they never need to re-resolve.

```rust
pub struct ResolvedPack {
    pub pack: Pack,
    pub source: PackSource,
}

pub enum PackSource {
    Registry { registry_url: String },
    Local { path: String },
    Git { url: String, rev: Option<String> },
}
```

### `PackExtensions`

CLI-specific extension configuration embedded in a pack manifest. Adapters ignore keys they don't understand, preserving forward compatibility.

```rust
pub struct PackExtensions {
    pub claude_code: Option<serde_json::Value>,
    pub gemini_cli: Option<serde_json::Value>,
    pub codex_cli: Option<serde_json::Value>,
}
```

### `Profile`

A named set of installed packs. Stored as `~/.packweave/profiles/<name>.toml`. One profile is active at a time, tracked in `~/.packweave/config.toml`.

```rust
pub struct Profile {
    pub name: String,
    pub packs: Vec<InstalledPack>,
}

pub struct InstalledPack {
    pub name: String,
    pub version: semver::Version,  // resolved, exact
    pub source: PackSource,        // Registry, Local, or Git
}
```

### Secrets and environment variables

Packs never store secret values. Instead they declare env var metadata (required/secret/description). Adapters write only env var references into CLI config files (for example, `${VAR}`, `$VAR`, or `bearer_token_env_var = "VAR"` depending on the CLI).

### `CliAdapter` trait

The central abstraction. Every supported CLI implements this trait. The core calls these methods; it never touches CLI config files directly.

Adapters ignore unknown `extensions.<cli>` keys to preserve forward compatibility. Packs can add future CLI-specific fields without breaking older weave versions.

```rust
pub trait CliAdapter: Send + Sync {
    /// Human-readable name, e.g. "Claude Code"
    fn name(&self) -> &str;

    /// Whether this CLI appears to be installed on the system
    fn is_installed(&self) -> bool;

    /// Root config directory for this CLI
    fn config_dir(&self) -> PathBuf;

    /// Apply a pack's contributions to this CLI's config.
    /// Must be idempotent — calling twice has the same effect as calling once.
    fn apply(&self, pack: &ResolvedPack) -> Result<()>;

    /// Remove all contributions made by a pack.
    /// Must leave user's manual edits untouched.
    fn remove(&self, pack_name: &str) -> Result<()>;

    /// Verify the CLI's current config is consistent with installed packs.
    /// Returns a list of issues for `weave doctor`.
    fn diagnose(&self) -> Result<Vec<DiagnosticIssue>>;
}
```

`apply()` must be **idempotent**. If a pack is already applied, calling `apply()` again must produce the same result without duplication or error.

`remove()` must be **surgical**. It removes only what `apply()` wrote, identified by tagged delimiters or a manifest of written paths. User edits to the same files are preserved.

### `Resolver`

Takes the current profile's pack list plus their dependency declarations and produces a flat `InstallPlan` — an ordered list of packs to install or remove, with exact pinned versions and no conflicts.

For v1, conflict resolution is simple: if two packs require incompatible versions of a dependency, resolution fails with a clear error. No automatic upgrade or compromise.

```rust
pub struct InstallPlan {
    pub to_install: Vec<(String, semver::Version)>,
    pub to_remove: Vec<String>,
    pub already_satisfied: Vec<String>,
}
```

### `Store`

Manages `~/.packweave/packs/`. Responsible for:

- Downloading pack archives from a URL
- Verifying SHA256 checksums
- Extracting to `~/.packweave/packs/<name>/<version>/`
- Evicting old versions when no longer referenced

The store never deletes a pack version that is referenced by any profile's lock file.

### `Registry` trait

```rust
pub trait Registry: Send + Sync {
    fn search(&self, query: &str) -> Result<Vec<PackSummary>>;
    fn fetch_metadata(&self, name: &str) -> Result<PackMetadata>;
    fn fetch_version(&self, name: &str, version: &semver::Version) -> Result<PackRelease>;
    fn publish(&self, archive: &Path, token: &str) -> Result<()>;
}
```

The default implementation (`GitHubRegistry`) reads a JSON index from the `PackWeave/registry` GitHub repo and resolves download URLs to GitHub Releases assets.

### Upstream MCP registries

The pack registry is distinct from MCP server registries. Packs may reference servers listed in upstream registries (official MCP Registry, Smithery, or other indexes), but weave does not attempt to curate a server list. Adapters only care about the resolved server definitions included in a pack.

-----

## Claude Code adapter — design detail

Claude Code stores configuration across user and project scopes:

```
~/.claude.json            User-scope MCP servers
.mcp.json                 Project-scope MCP servers
~/.claude/settings.json   User-scope settings + hooks
.claude/settings.json     Project-scope settings + hooks
~/.claude/commands/       Slash commands (.md files)
~/.claude/CLAUDE.md       Global system prompt / instructions
```

### MCP servers

`~/.claude.json` and `.mcp.json` contain a `mcpServers` key. The adapter merges pack-defined servers into this map (per scope). To track ownership, it maintains a sidecar file at `~/.claude/.packweave_manifest.json`:

```json
{
  "servers": {
    "puppeteer": "webdev",
    "filesystem": "webdev"
  },
  "commands": {
    "webdev__review.md": "webdev"
  },
  "prompt_blocks": ["webdev"]
}
```

On removal, the adapter consults this manifest to know exactly what to undo.

### Slash commands

Pack commands are copied (not symlinked) into `~/.claude/commands/` with a namespaced filename: `<pack-name>__<command>.md`. The double underscore is the ownership delimiter.

Symlinks are avoided because some editors and sync tools (iCloud, Dropbox) don't handle them well.

### System prompt

The adapter appends pack prompt content from `prompts/claude.md` (or `prompts/system.md` as a fallback) to `CLAUDE.md` between tagged delimiters:

```markdown
<!-- packweave:begin:webdev -->
You are an expert web developer...
<!-- packweave:end:webdev -->
```

On removal, everything between the tags (inclusive) is deleted. The rest of `CLAUDE.md` is untouched.

### Settings fragments

Pack settings (`settings/claude.json`) are deep-merged into `~/.claude/settings.json` or `.claude/settings.json` (matching the target scope). On removal, only the keys originally written by this pack (recorded in the manifest) are deleted. If the user has manually modified a key that a pack wrote, the adapter leaves it alone and emits a warning.

### Hooks

Hooks are deferred until v0.3. When introduced, they will live under `extensions.claude_code.hooks` and require explicit opt-in (for example, `--allow-hooks`).

-----

## Gemini CLI adapter — design detail

Gemini CLI stores MCP configuration in JSON:

```
~/.gemini/settings.json   User-scope settings + MCP servers
.gemini/settings.json     Project-scope settings + MCP servers
~/.gemini/GEMINI.md       Global system prompt / instructions
```

### MCP servers

The adapter merges pack-defined servers into the `mcpServers` key of `settings.json`. Ownership is tracked in a sidecar file at `~/.gemini/.packweave_manifest.json` (same structure as the Claude Code manifest). On removal, the adapter consults this manifest to undo only what it wrote.

### System prompt

Prompt content from `prompts/gemini.md` (or `prompts/system.md` as fallback) is appended to `GEMINI.md` between the same tagged delimiters used by the Claude Code adapter:

```markdown
<!-- packweave:begin:webdev -->
...
<!-- packweave:end:webdev -->
```

### Settings fragments

Pack settings (`settings/gemini.json`) are deep-merged into `settings.json`. On removal, only keys originally written by this pack are deleted, as recorded in the manifest.

-----

## Codex CLI adapter — design detail

Codex CLI uses JSON configuration and has no project-scope equivalent:

```
~/.codex/config.json      User-scope settings (model, approvalMode, providers, …)
~/.codex/AGENTS.md        Global instruction file (Codex merges ~/.codex/, repo root, cwd)
```

MCP server support is not available in Codex CLI; the adapter handles **prompts** and **settings** only. Prompt content from `prompts/codex.md` (or `prompts/system.md` as fallback) is appended to `~/.codex/AGENTS.md` using `<!-- packweave:begin/end:name -->` delimiters. Settings from `settings/codex.json` are deep-merged into `~/.codex/config.json` with per-key snapshots stored in the sidecar manifest for safe removal.

-----

## Pack archive format

Packs are distributed as `.tar.gz` archives. The registry index entry for each version includes:

```json
{
  "version": "1.2.0",
  "url": "https://github.com/PackWeave/registry/releases/download/...",
  "sha256": "abc123...",
  "size_bytes": 4096,
  "dependencies": {
    "other-pack": "^1.0.0"
  }
}
```

The store always verifies the SHA256 before extracting. A failed verification aborts the install.

-----

## State files

|File                                 |Purpose                                           |
|-------------------------------------|--------------------------------------------------|
|`~/.packweave/config.toml`           |Active profile name, registry URL, auth token path|
|`~/.packweave/profiles/<n>.toml`     |Installed pack list for a profile                 |
|`~/.packweave/locks/<n>.lock`        |Pinned exact versions for a profile               |
|`~/.packweave/packs/<name>/<ver>/`   |Extracted pack contents                           |
|`~/.claude/.packweave_manifest.json` |Tracks what weave wrote in Claude Code config     |
|`~/.gemini/.packweave_manifest.json` |Tracks what weave wrote in Gemini CLI config      |
|`~/.codex/.packweave_manifest.json`  |Tracks what weave wrote in Codex CLI config       |

-----

## Error handling

All errors use `thiserror` for structured types and `anyhow` for propagation in CLI handlers. Errors shown to the user are formatted by a top-level handler in `main.rs` — they are always actionable (what went wrong + what to do about it).

Panics are not used for recoverable errors. `unwrap()` and `expect()` are only acceptable for truly invariant conditions, with a comment explaining why.

-----

## Testing strategy

- **Unit tests** live alongside the module they test (`#[cfg(test)]` blocks).
- **Integration tests** live in `tests/` and operate against a temporary `~/.packweave/` directory created per-test.
- **Adapter tests** use fixture CLI config directories (checked in under `tests/fixtures/`) to verify that apply/remove produce exactly the expected output.
- The registry is mocked in tests — no network calls in CI.

-----

## What is explicitly out of scope

- GUI or TUI — weave is a CLI tool only
- MCP server execution or sandboxing — weave installs config, it does not run MCP servers
- MCP server discovery or recommendation — that's the registry's job, not the core tool's
- IDE plugins — out of scope for v1 and v2
- Windows support — weave targets macOS and Linux; Windows is not tested in CI. The Claude Code adapter works on Windows as a best-effort target since Claude Code itself supports Windows, but other adapters (Gemini CLI, Codex CLI) do not have Windows-compatible CLIs
