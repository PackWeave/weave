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
  Lock                     ← acquires ~/.packweave/.lock (prevents concurrent mutations)
       │
       ▼
  Registry client          ← fetches pack metadata + inline file content
       │
       ▼
  Resolver                 ← resolves semver, checks conflicts, builds install plan
       │
       ▼
  Store                    ← writes inline files to ~/.packweave/packs/
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
  error.rs                 Unified error types via thiserror.
  util.rs                  Shared helpers (file ops, path resolution, etc.)

  cli/                     Clap command definitions and thin handler functions.
    mod.rs                 CLI dispatch and shared argument types.
    auth.rs                Registry authentication (login/logout/status).
    diagnose.rs            Config drift detection and health reporting.
    init.rs                Scaffold a new pack directory.
    install.rs             Thin wrapper → core::install.
    list.rs                Show installed packs with versions and scope.
    profile.rs             Profile create/list/delete/add.
    publish.rs             Publish a pack to the registry.
    remove.rs              Remove a pack and clean up config entries.
    search.rs              Pack registry and MCP Registry search.
    sync.rs                Reapply active profile to all adapters.
    tap.rs                 Community tap add/list/remove.
    update.rs              Thin wrapper → core::update.
    use_profile.rs         Thin wrapper → core::use_profile.

  core/                    Business logic — no I/O to CLI config files here.
    mod.rs                 Module re-exports.
    config.rs              Global weave config (~/.packweave/config.toml).
    credentials.rs         Token storage, retrieval, and validation.
    conflict.rs            Tool-level conflict detection across installed packs.
    install.rs             Install orchestration (registry + local).
    lockfile.rs            Lock file: read/write, version pinning.
    lock.rs                Advisory file lock for concurrency safety.
    mcp_registry.rs        Upstream MCP Registry client (weave search --mcp).
    pack.rs                Pack manifest: parsing, validation, the Pack struct.
    profile.rs             Profile: read/write, active profile tracking.
    publish.rs             Publish orchestration: file collection, version check, GitHub PR creation.
    registry.rs            Registry trait, GitHubRegistry, and CompositeRegistry.
    resolver.rs            Dependency graph construction and semver resolution.
    store.rs               Local pack cache: download, extract, verify, evict.
    update.rs              Update orchestration (version comparison + apply).
    use_profile.rs         Profile switch orchestration (diff + remove + apply).

  adapters/                CLI-specific config read/write — no business logic here.
    mod.rs                 CliAdapter trait, ApplyOptions, AdapterId, DiagnosticIssue.
    claude_code.rs         Claude Code adapter (~/.claude/).
    codex_cli.rs           Codex CLI adapter (~/.codex/).
    gemini_cli.rs          Gemini CLI adapter (~/.gemini/).
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
    /// Required for stdio transport; None for http.
    pub command: Option<String>,
    pub args: Vec<String>,
    /// Required for http transport; None for stdio.
    pub url: Option<String>,
    /// Optional HTTP headers (e.g. Authorization). Only used for http transport.
    pub headers: Option<HashMap<String, String>>,
    pub transport: Option<Transport>,  // Stdio (default) or Http
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
    /// Stable machine identifier for this adapter.
    /// Used for internal logic (target mapping, diagnose attribution).
    fn id(&self) -> AdapterId;

    /// Human-readable name, e.g. "Claude Code"
    fn name(&self) -> &str;

    /// Whether this CLI appears to be installed on the system
    fn is_installed(&self) -> bool;

    /// Root config directory for this CLI
    fn config_dir(&self) -> PathBuf;

    /// Apply a pack's contributions to this CLI's config.
    /// Must be idempotent — calling twice has the same effect as calling once.
    /// `options` controls optional behaviours like hooks application.
    fn apply(&self, pack: &ResolvedPack, options: &ApplyOptions) -> Result<()>;

    /// Remove all contributions made by a pack.
    /// Must leave user's manual edits untouched.
    /// Returns a list of non-fatal warnings (e.g. project-scope cleanup failures).
    fn remove(&self, pack_name: &str) -> Result<Vec<String>>;

    /// Verify the CLI's current config is consistent with installed packs.
    /// Returns a list of issues for `weave diagnose`.
    fn diagnose(&self) -> Result<Vec<DiagnosticIssue>>;

    /// Returns the set of pack names this adapter is currently tracking
    /// (i.e., has contributions for in its sidecar manifest).
    fn tracked_packs(&self) -> Result<HashSet<String>>;
}

pub struct ApplyOptions {
    /// When true, hooks declared in the pack manifest are written to the
    /// CLI config. Hooks execute arbitrary shell commands, so they require
    /// explicit user consent via the `--allow-hooks` flag.
    pub allow_hooks: bool,
}

pub enum AdapterId {
    ClaudeCode,
    GeminiCli,
    CodexCli,
}

pub struct DiagnosticIssue {
    pub severity: Severity,  // Warning or Error
    pub message: String,
    pub suggestion: Option<String>,
    pub pack: Option<String>,
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

- Writing pack files from the inline `files` map in `PackRelease` to `~/.packweave/packs/<name>/<version>/`
- Path-validating all keys in the `files` map (rejects absolute paths, `..` components, Windows drive prefixes)
- Evicting old versions when no longer referenced

Uses an atomic staging pattern: files are written to a `.tmp` directory first, then renamed to the final destination so a failure never leaves a partial cache entry.

The store never deletes a pack version that is referenced by any profile's lock file.

### `Registry` trait

```rust
pub trait Registry: Send + Sync {
    fn search(&self, query: &str) -> Result<Vec<PackSummary>>;
    fn fetch_metadata(&self, name: &str) -> Result<PackMetadata>;
    fn fetch_version(&self, name: &str, version: &semver::Version) -> Result<PackRelease>;
    fn publish(&self, pack: &Pack, files: &BTreeMap<String, Vec<u8>>, token: &str) -> Result<PublishResult>;
}
```

The default implementation (`GitHubRegistry`) uses a two-tier sparse index against the `PackWeave/registry` GitHub repo:

- **`{base_url}/index.json`** — lightweight catalog fetched once for `weave search` and `weave list`. Contains only pack names, descriptions, and latest versions. Cached in-process after first fetch.
- **`{base_url}/packs/{name}.json`** — full pack metadata fetched on demand when installing or resolving a specific pack. Contains all versions with their complete file content embedded inline as a flat map of relative path → file content. Cached per-pack after first fetch.

Key structs:

```rust
/// Entry in the lightweight search index.
struct PackListing {
    name: String,
    description: String,
    keywords: Vec<String>,
    latest_version: semver::Version,
}

/// Full metadata for one pack, fetched on demand.
pub struct PackMetadata {
    pub name: String,
    pub description: String,
    pub authors: Vec<String>,
    pub license: Option<String>,
    pub repository: Option<String>,
    pub keywords: Vec<String>,
    pub versions: Vec<PackRelease>,
}
```

This design keeps `weave install` fast regardless of registry size — clients never download more metadata than they need. See `docs/REGISTRY.md` for the full protocol specification.

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

`~/.claude.json` contains the `mcpServers` key (user scope). When `--project` is passed to `weave install`, the adapter also writes servers to `.mcp.json` in the current directory (project scope).

To track ownership, the adapter maintains a sidecar file at `~/.claude/.packweave_manifest.json`:

```json
{
  "servers": {
    "puppeteer": "webdev",
    "filesystem": "webdev"
  },
  "commands": {
    "webdev__review.md": "webdev"
  },
  "prompt_blocks": ["webdev"],
  "project_dirs": {
    "webdev": ["/Users/dev/my-project"]
  }
}
```

`project_dirs` records the absolute (canonicalized) paths of project roots where `--project` installs have been applied. On removal, the adapter consults this manifest to clean up both user-scope and all project-scope state — regardless of the current working directory. Failed cleanups are retained for retry on the next `weave remove`.

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

Hooks allow packs to register shell commands that run at Claude Code lifecycle events (e.g. `PreToolUse`, `PostToolUse`). Because hooks execute arbitrary code, they require explicit user consent.

Pack manifests declare hooks under `extensions.claude_code.hooks`:

```toml
[extensions.claude_code.hooks]
PreToolUse = [{ matcher = "Bash", command = "echo pre-check" }]
```

The adapter writes hooks to `~/.claude/settings.json` under the `hooks` key only when the user passes `--allow-hooks` to `install`, `sync`, or `use`. Without the flag, the CLI prints a notice that hooks were skipped. Hooks are tracked in the sidecar manifest for surgical removal.

Gemini CLI and Codex CLI do not support hooks. If a pack declares hooks for these CLIs, the adapter logs a warning and skips them.

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

Codex CLI uses TOML configuration:

```
~/.codex/config.toml               User-scope MCP servers + settings
.codex/config.toml                 Project-scope MCP servers + settings
~/.codex/AGENTS.md                 User-scope system prompt / instructions
~/.codex/skills/                   Skill files (.md), Codex's slash-command equivalent
~/.codex/.packweave_manifest.json  Ownership tracking sidecar
```

### MCP servers

The adapter merges pack-defined servers into the `[mcp_servers.<name>]` table in `config.toml`. Each server becomes a TOML table entry:

```toml
[mcp_servers.filesystem]
command = "npx"
args = ["-y", "@modelcontextprotocol/server-filesystem"]
enabled = true
```

Ownership is tracked in `~/.codex/.packweave_manifest.json` (same JSON schema as the Claude Code manifest). On removal, the adapter consults this manifest to undo only what it wrote.

### Skills

Pack skill files are copied into `~/.codex/skills/` with namespaced filenames: `<pack-name>__<skill>.md`. This mirrors how Claude Code commands use `~/.claude/commands/`.

### System prompt

Prompt content from `prompts/codex.md` (or `prompts/system.md` as fallback) is appended to `~/.codex/AGENTS.md` between tagged delimiters:

```markdown
<!-- packweave:begin:webdev -->
...
<!-- packweave:end:webdev -->
```

### Settings fragments

Pack settings (`settings/codex.toml`, or `settings/codex.json` as fallback) are merged (top-level keys only) into `config.toml`. On removal, only keys originally written by the pack are removed or restored to their pre-install values — provided the user has not modified them since install. If a key was modified, the adapter warns and leaves it in place.

### Manifest atomicity

The adapter saves the manifest after each individual mutation step (servers, skills, prompts, settings). If `apply()` fails partway through, the manifest accurately reflects what was written to disk, so `remove()` and `diagnose()` can still clean up correctly.

-----

## Pack content format

Packs are distributed as inline JSON — file content is embedded directly in `packs/{name}.json`.
Each version entry contains a `files` map of relative path → file content:

```json
{
  "version": "1.2.0",
  "files": {
    "pack.toml": "[pack]\nname = \"my-pack\"\n...",
    "prompts/system.md": "You are...",
    "commands/review.md": "# Review\n..."
  },
  "dependencies": {
    "other-pack": "^1.0.0"
  }
}
```

The store writes each entry directly to `~/.packweave/packs/{name}/{version}/` after
path-validating the key (rejects absolute paths, `..` components, Windows drive prefixes).
Trust is provided by TLS and GitHub as the content host. No tarballs, no release artifacts,
no SHA256 ceremony.

-----

## State files

|File                                 |Purpose                                           |
|-------------------------------------|--------------------------------------------------|
|`~/.packweave/config.toml`           |Active profile name, registry URL, auth token path|
|`~/.packweave/credentials`          |Registry auth token (written by `weave auth login`, 0o600)|
|`~/.packweave/profiles/<n>.toml`     |Installed pack list for a profile                 |
|`~/.packweave/locks/<n>.lock`        |Pinned exact versions for a profile               |
|`~/.packweave/.lock`                |Advisory file lock preventing concurrent mutations |
|`~/.packweave/packs/<name>/<ver>/`   |Inline pack file contents written on install      |
|`~/.claude/.packweave_manifest.json` |Tracks what weave wrote in Claude Code config     |
|`~/.gemini/.packweave_manifest.json` |Tracks what weave wrote in Gemini CLI config      |
|`~/.codex/.packweave_manifest.json`  |Tracks what weave wrote in Codex CLI config       |

-----

## Error handling

All errors use `thiserror` for structured types and `anyhow` for propagation in CLI handlers. Errors shown to the user are formatted by a top-level handler in `main.rs` — they are always actionable (what went wrong + what to do about it).

Panics are not used for recoverable errors. `unwrap()` and `expect()` are only acceptable for truly invariant conditions, with a comment explaining why.

-----

## Schema versioning

Every persisted file format carries a `schema_version` integer field. This enables forward-compatible rejection: if a file was written by a newer weave version with a schema the current build does not understand, weave refuses to load it with a clear "please upgrade" error rather than silently misinterpreting the data.

**Versioned formats:**

| File | Constant | Location |
|------|----------|----------|
| `pack.toml` | `CURRENT_PACK_SCHEMA_VERSION` | `core/pack.rs` |
| Profile lock files (`*.lock`) | `CURRENT_LOCKFILE_SCHEMA_VERSION` | `core/lockfile.rs` |
| Registry `index.json` | `CURRENT_REGISTRY_SCHEMA_VERSION` | `core/registry.rs` |
| Registry `packs/{name}.json` | `CURRENT_REGISTRY_SCHEMA_VERSION` | `core/registry.rs` |
| Adapter tracking files (`.packweave_manifest.json`) | `CURRENT_MANIFEST_SCHEMA_VERSION` | `adapters/mod.rs` |

**Rules:**

1. **Default is 1.** When `schema_version` is absent, serde defaults it to `1`. This provides backward compatibility with files written before versioning was added.
2. **Reject, never guess.** If `schema_version > CURRENT_*_SCHEMA_VERSION`, the load function returns `WeaveError::SchemaVersionTooNew`. No fallback parsing is attempted.
3. **Bump the constant, not the code.** When a format change ships, bump the relevant `CURRENT_*` constant. Older clients will reject the new format automatically.
4. **Registry index supports two shapes.** The `index.json` file accepts both a versioned envelope (`{"schema_version": N, "packs": {…}}`) and the legacy flat format (`{"pack-name": {…}, …}`) for backward compatibility with older registries and taps.

-----

## Testing strategy

- **Unit tests** live alongside the module they test (`#[cfg(test)]` blocks).
- **Integration tests** live in `tests/` and operate against temporary directories created per-test via `TempDir`.
- **Adapter tests** create isolated home and project directories. Store isolation is achieved via the `WEAVE_TEST_STORE_DIR` environment variable.
- The registry is mocked in tests — no network calls in CI.

-----

## Environment variables

|Variable                |Purpose                                                        |
|------------------------|---------------------------------------------------------------|
|`WEAVE_TOKEN`           |Override credentials file for registry authentication (CI/automation)|
|`WEAVE_TEST_STORE_DIR`  |Overrides `~/.packweave/` root (used in tests)                 |
|`WEAVE_REGISTRY_URL`    |Overrides registry URL in Config (used in E2E tests, PR #84)  |

-----

## What is explicitly out of scope

- GUI or TUI — weave is a CLI tool only
- MCP server execution or sandboxing — weave installs config, it does not run MCP servers
- MCP server discovery or recommendation — that's the registry's job, not the core tool's
- IDE plugins — out of scope for v1 and v2
- Windows support — weave targets macOS and Linux; Windows is not tested in CI. The Claude Code adapter works on Windows as a best-effort target since Claude Code itself supports Windows, but other adapters (Gemini CLI, Codex CLI) do not have Windows-compatible CLIs
