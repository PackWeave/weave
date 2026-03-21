# Milestone 3 Behavioral Spec (v0.2)

This document defines the **observable behavior** of every feature shipped in Milestone 3. It is the contract between design and implementation: code is done when all scenarios in this spec pass, not when the logic compiles.

Architecture and module boundaries remain as defined in `docs/ARCHITECTURE.md`. This document adds the behavioral layer on top.

---

## How to read this document

Each feature section contains:

- **Goal** — one sentence on what this feature does for the user
- **Design decisions** — explicit choices that must be answered before implementation begins, marked `[DECISION]`
- **Contracts** — Given/When/Then scenarios that must be verified by tests
- **Invariants** — properties that hold across all scenarios
- **Required test scenarios** — the minimum set of tests that must exist before the feature is considered done

A `[DECISION]` is not a suggestion. Implementation must not begin until the owner has picked one option and removed the marker.

---

## Pre-implementation gate

**No PR may be opened for any M3 feature until all three items below are checked off.** These are factual unknowns — the spec contains best-guess assumptions that will produce broken code if wrong. Verify against live documentation or the running CLI before writing a line of implementation.

- [ ] **Codex CLI config schema confirmed** — verify the server table key (`mcp_servers`?), whether a `system_prompt` key exists and how it behaves, and what top-level keys a `settings/codex.toml` fragment should be merged into. If `system_prompt` does not exist, prompts are a no-op for the Codex adapter in v0.2.
- [ ] **MCP Registry API shape confirmed** — verify the search endpoint URL, the response JSON structure (field names for server name, description, package identifier), pagination behaviour, and any rate limits. The shape in section 2 is illustrative only.
- [ ] **Env var reference formats confirmed per CLI** — verify the exact string that each CLI expects in its config for an env var reference. A wrong format (e.g. `$VAR` vs `${VAR}`) silently fails at AI CLI invocation time, which is hard to debug after the fact. Formats to confirm: Claude Code (`~/.claude.json` env field), Gemini CLI (`settings.json` env field), Codex CLI (`config.toml` env field).

Once all three are checked, update this document with the verified values before opening any PR.

---

## 1. Codex CLI Adapter

### Goal

Apply and remove pack contributions (MCP servers, prompts, settings) to/from the Codex CLI's TOML config at `~/.codex/config.toml` and `.codex/config.toml`, following the same ownership and idempotency invariants as the Claude Code and Gemini CLI adapters.

### Scope

- **In scope:** servers, prompts (via `prompts/codex.md` or `prompts/system.md`), settings (via `settings/codex.toml`)
- **Out of scope for v0.2:** Windows support (Codex CLI does not have a Windows build). The adapter must compile on Windows but `is_installed()` returns `false` on non-Unix platforms.

### Config schema

Codex CLI uses a TOML config file. The relevant config keys must be confirmed against the live Codex CLI documentation before implementing prompts or settings — the server table name below is the best current assumption:

```toml
# ~/.codex/config.toml

[mcp_servers.filesystem]
command = "npx"
args = ["-y", "@modelcontextprotocol/server-filesystem", "/tmp"]

[mcp_servers.puppeteer]
command = "npx"
args = ["-y", "@modelcontextprotocol/server-puppeteer"]
```

> **Action required before implementation:** Confirm the exact key names for servers (`mcp_servers`?), the system prompt key (if any), and any settings keys the pack's `settings/codex.toml` fragment should be merged into. If Codex CLI does not support a `system_prompt` config key, prompts are a no-op for this adapter in v0.2.

### Prompts — raw string operations only

The Codex adapter must **not** parse and re-serialize the TOML file when managing prompt content. TOML libraries (including the `toml` crate) silently drop comments during round-trip serialization. A parse/serialize cycle would destroy the tagged blocks on the first settings write.

Prompt blocks are managed as raw string find-and-replace on the file content, never through a TOML parser — the same strategy used for `CLAUDE.md` in the Claude Code adapter:

```
# packweave:begin:webdev
system_prompt = "You are an expert web developer..."
# packweave:end:webdev
```

This means prompt writes are a separate code path from settings writes. Settings uses the TOML parser; prompts do not.

> If Codex CLI stores system prompts in a separate file (not `config.toml`), use that file exclusively for prompt management and avoid the comment-block pattern entirely.

### Settings

Pack settings (`settings/codex.toml`) are deep-merged into `~/.codex/config.toml`.

**Implementation note:** Unlike Claude Code (which uses `serde_json::Value` with a straightforward JSON merge), TOML deep-merge requires manually walking `toml::Value::Table` entries — the `toml` crate has no built-in merge. Budget extra implementation effort here. The merge strategy is otherwise identical to the Claude Code adapter: snapshot the pre-apply values and restore them surgically on remove.

### Sidecar manifest

The Codex adapter uses `~/.codex/.packweave_manifest.json` with the same JSON schema as the Claude Code adapter:

```json
{
  "servers": { "filesystem": "webdev" },
  "prompt_blocks": ["webdev"],
  "settings": {}
}
```

### Contracts

**apply() — servers**

- Given a pack with `servers = [{name = "fs", command = "npx", args = [...]}]` and an empty `~/.codex/config.toml`
- When `apply()` is called
- Then `~/.codex/config.toml` contains `[mcp_servers.fs]` with the correct `command` and `args`
- And `~/.codex/.packweave_manifest.json` records `"fs": "<pack_name>"` under `servers`

**apply() — servers idempotent**

- Given the pack is already applied
- When `apply()` is called a second time
- Then `~/.codex/config.toml` does not contain duplicate `[mcp_servers.fs]` entries
- And the manifest is unchanged

**apply() — server collision**

- Given a `[mcp_servers.X]` key already exists in `config.toml` and is not owned by this pack's manifest
- When `apply()` is called
- Then `apply()` returns `ApplyFailed` with a message naming the conflicting key and its owner

**apply() — prompts**

- Given a pack with a non-empty `prompts/codex.md`
- When `apply()` is called
- Then `~/.codex/config.toml` contains the prompt content between `# packweave:begin:<pack>` and `# packweave:end:<pack>` markers

**remove() — servers**

- Given the pack was previously applied
- When `remove()` is called
- Then all `[mcp_servers.X]` entries owned by this pack are deleted
- And entries not owned by this pack are untouched
- And the manifest entries for this pack are deleted

**remove() — prompts**

- Given the pack's prompt block exists in `config.toml`
- When `remove()` is called
- Then the tagged block (begin marker, content, end marker) is removed
- And all content outside the markers is untouched

**remove() — pack not applied**

- Given the manifest has no record of this pack
- When `remove()` is called
- Then `remove()` returns `Ok(())` without modifying any file

**is_installed()**

- On Linux/macOS: returns `true` iff `~/.codex/` exists
- On Windows: always returns `false`

### Invariants

1. Every `[mcp_servers.X]` written by `apply()` is recorded in the manifest before `apply()` returns.
2. `remove()` never deletes a key not recorded in the manifest for this pack.
3. Prompt writes use raw string operations; settings writes use the TOML parser. These must never be mixed.
4. Manifest records are not consumed until after a successful file write (same pattern as Claude Code adapter — `get().cloned()` then deferred `remove()`).

### Required test scenarios

- [ ] `apply()` writes servers to an empty config
- [ ] `apply()` is idempotent (double-apply produces no duplicates)
- [ ] `apply()` fails with `ApplyFailed` when a server key is already owned by a different pack
- [ ] `apply()` writes prompt block between tagged markers
- [ ] `remove()` restores config to pre-apply state (servers and prompts)
- [ ] `remove()` on a pack not in the manifest returns `Ok(())` without file changes
- [ ] Manifest record is not consumed until after a successful write

---

## 2. `weave search` against the official MCP Registry

### Goal

Let users discover MCP servers from the official MCP Registry directly from the weave CLI, as a separate mode from weave pack search.

### Decision: `--mcp` flag ✓

`weave search <query>` queries the weave pack registry (existing M2 behavior, unchanged).
`weave search --mcp <query>` queries `registry.modelcontextprotocol.io`.

This is purely additive — no breaking change, no deduplication complexity, no semantic mismatch between the two result types.

### Output distinction: servers vs packs

The MCP Registry returns **servers** (e.g. `@modelcontextprotocol/server-filesystem`). weave installs **packs** (bundles of servers). These are different things. `--mcp` results must be clearly labelled as MCP servers, not weave packs, and the output must include a note explaining the distinction:

```
MCP server results for 'filesystem':

  filesystem
    Local filesystem access
    Package: @modelcontextprotocol/server-filesystem (npm)
    Source:  https://github.com/modelcontextprotocol/servers

Note: these are MCP servers, not weave packs. To use a server with weave,
find or create a weave pack that includes it (weave search filesystem).
```

### MCP Registry API

The official MCP Registry exposes a REST API. The relevant endpoint:

```
GET https://registry.modelcontextprotocol.io/servers?q=<query>
```

> **Action required before implementation:** Confirm the live API response shape, pagination behaviour, and rate limits. The response structure below is illustrative and must be verified.

```json
{
  "servers": [
    {
      "id": "...",
      "name": "filesystem",
      "description": "Local filesystem access",
      "vendor": "...",
      "githubUrl": "...",
      "packages": [{ "packageType": "npm", "name": "@modelcontextprotocol/server-filesystem" }]
    }
  ]
}
```

### Contracts

**weave search (weave registry, unchanged)**

- Given the weave registry contains a pack named "webdev"
- When `weave search web` is run
- Then output lists "webdev" with its description
- And no MCP Registry network calls are made

**weave search --mcp**

- Given the MCP Registry returns results for "filesystem"
- When `weave search --mcp filesystem` is run
- Then output lists results with server names, descriptions, and package identifiers
- And output includes a note distinguishing MCP servers from weave packs
- And no weave registry calls are made

**weave search --mcp (no results)**

- Given the MCP Registry returns an empty result set
- When `weave search --mcp unknownxyz` is run
- Then output says "No MCP servers found matching 'unknownxyz'" (or equivalent)
- And exits with code 0

**weave search --mcp (network error)**

- Given the MCP Registry is unreachable (mocked)
- When `weave search --mcp filesystem` is run
- Then output states the registry is unreachable and suggests checking network connectivity
- And exits with a non-zero code

### Invariants

1. `weave search` (no `--mcp`) is identical to M2 behavior — no regressions.
2. `--mcp` makes no weave registry calls; `weave search` makes no MCP Registry calls.
3. Tests for `--mcp` mock the HTTP client; no real network calls in CI.

### Required test scenarios

- [ ] `weave search` queries weave registry (unchanged M2 behavior)
- [ ] `weave search --mcp` returns and formats MCP Registry results with server/pack distinction note
- [ ] `weave search --mcp` handles empty result set gracefully
- [ ] `weave search --mcp` surfaces a clear error on network failure (mocked)

---

## 3. `weave update`

### Goal

Update one or all installed packs to the latest version satisfying their current major version constraint, re-applying the updated pack to all installed CLI adapters.

### Decision: no-args updates all ✓

`weave update` (no args) updates all installed packs in the active profile. `weave update <pack>` updates one. This matches `brew upgrade` / `npm update` expectations.

### Decision: stay within current major by default ✓

`weave update` resolves the latest version within the pack's current major (i.e. `^<installed_major>.0.0`). This prevents accidental breaking upgrades — semver major bumps are breaking changes by definition.

`weave update <pack>@latest` opts into a cross-major upgrade. This is explicit and intentional.

### Update flow

`weave update` reuses `Resolver::plan_install()` with `version_req = Some("^<installed_major>.x")` — no new resolver method is needed. The existing `plan_install()` already handles the "already at latest" case correctly.

```
weave update [pack-name]
       │
       ▼
  Resolver::plan_install(name, Some("^major.x"), profile)
       │  (returns InstallPlan; to_install is empty if already at latest)
       ▼
  Store::fetch(new version)
       │
       ▼
  Adapters::remove(old version)    ← for each installed adapter
       │
       ▼
  Adapters::apply(new version)     ← collect-and-continue (same as install)
       │
       ▼
  Profile + LockFile updated       ← only after remove+apply attempted
```

### Error handling: collect-and-continue (same as install)

`weave update` follows the same collect-and-continue strategy as `install.rs`. If an adapter's `apply()` fails, the pack is still recorded at the new version with a warning. This is intentionally consistent — there is no special rollback for update. If the state is inconsistent after a failed update, `weave doctor` (M4) is the resolution path.

The old pack version is not evicted from the store until after the update completes (eviction is a separate concern handled by Store GC, not by the update command).

### Contracts

**weave update <pack> — newer version available**

- Given pack "webdev" is installed at `1.0.0` and `1.1.0` is available in the registry
- When `weave update webdev` is run
- Then the store contains `webdev/1.1.0/`
- And the active profile records `webdev` at `1.1.0`
- And the lock file pins `webdev` at `1.1.0`
- And all adapter configs reflect the new version's servers/prompts/settings

**weave update <pack> — already up to date**

- Given "webdev" is installed at the latest version within its major
- When `weave update webdev` is run
- Then output says "webdev is already up to date" (or equivalent)
- And no files are modified

**weave update <pack> — pack not installed**

- Given "webdev" is not in the active profile
- When `weave update webdev` is run
- Then output states "webdev is not installed" with a suggestion to run `weave install webdev`
- And exits with a non-zero code

**weave update (no args)**

- Given multiple packs are installed, some at outdated versions
- When `weave update` is run
- Then all packs with newer versions available are updated
- And already-current packs are reported as up to date

**weave update <pack>@latest — cross-major upgrade**

- Given "webdev" is at `1.2.0` and `2.0.0` is available
- When `weave update webdev@latest` is run
- Then the pack is updated to `2.0.0`

**weave update — adapter apply fails**

- Given an adapter's `apply()` fails for the new version
- When `weave update` is run
- Then the pack is recorded at the new version in profile and lockfile
- And the failure is surfaced as a warning (consistent with install behavior)
- And the user is told to run `weave doctor` once available

### Invariants

1. The profile and lock file reflect what was *attempted*, not what fully succeeded — same invariant as install.
2. `weave update` with no packs installed is a no-op with a message, not an error.
3. The old store version is not deleted by the update command itself.

### Required test scenarios

- [ ] Update to newer version within same major: profile, lockfile, adapters all updated
- [ ] Already up to date: no-op with informational output
- [ ] Pack not installed: exits with non-zero and actionable message
- [ ] `weave update` (no args) updates all outdated packs, reports current ones
- [ ] `weave update <pack>@latest` upgrades across major versions
- [ ] Adapter failure: pack recorded at new version with warning (collect-and-continue)

---

## 4. `weave init`

### Goal

Scaffold a new pack directory with the required files and structure, so a pack author can start publishing without reading the schema docs.

### Decision: both in-place and subdirectory ✓

`weave init <name>` creates `<name>/` as a subdirectory of cwd.
`weave init` (no args) initializes the current directory using the directory name as the pack name.
This matches `git init` behavior.

### Decision: pre-filled defaults, no interactive prompts ✓

`weave init` pre-fills sensible defaults and marks TODOs. No stdin interaction. Easier to test; interactive CLIs require careful terminal handling that's out of scope for v0.2.

### Pack name validation

Pack names must match `[a-z0-9-]+` (enforced by `Pack::validate()` in `src/core/pack.rs`). `weave init` must validate the name at scaffold time — not silently create a `pack.toml` that fails validation on first use.

If the name contains uppercase or underscores, reject it immediately:

```
error: pack name 'MyPackName' is invalid
  Pack names must contain only lowercase letters, numbers, and hyphens.
  Try: weave init my-pack-name
```

### Scaffold output

```
<pack-name>/
  pack.toml            # Required. Pre-filled with sensible defaults.
  README.md            # Minimal template.
  prompts/
    system.md          # Empty. Prompt content goes here.
  settings/            # Empty directory (for CLI-specific settings fragments).
  commands/            # Empty directory (for Claude Code slash commands).
```

### pack.toml template

```toml
[pack]
name = "<pack-name>"
version = "0.1.0"
description = "TODO: describe what this pack does"
authors = ["TODO: your name <you@example.com>"]
license = "MIT"

[targets]
claude_code = true
gemini_cli = false
codex_cli = false
```

### Contracts

**weave init <name>**

- Given no directory named `<name>` exists in cwd
- And `<name>` matches `[a-z0-9-]+`
- When `weave init <name>` is run
- Then `<name>/pack.toml` exists with `name = "<name>"` and `version = "0.1.0"`
- And `<name>/prompts/system.md` exists (empty)
- And `<name>/settings/` and `<name>/commands/` directories exist
- And `<name>/README.md` exists
- And the generated `pack.toml` parses without error via `Pack::load()`

**weave init <name> — invalid name**

- Given `<name>` contains uppercase letters or underscores
- When `weave init <name>` is run
- Then the command exits with a non-zero code and a message explaining the naming rules
- And no files or directories are created

**weave init <name> — directory already exists**

- Given a directory named `<name>` already exists in cwd
- When `weave init <name>` is run
- Then the command exits with a non-zero code stating the directory already exists
- And no files are created or modified

**weave init (no args)**

- Given the current directory does not contain a `pack.toml`
- And the current directory name matches `[a-z0-9-]+`
- When `weave init` is run
- Then `pack.toml` is created in the current directory with `name = <current-dir-name>`

**weave init (no args) — pack.toml already exists**

- Given `pack.toml` already exists in the current directory
- When `weave init` is run
- Then the command exits with a non-zero code stating "pack.toml already exists"
- And no files are modified

### Invariants

1. `weave init` never overwrites existing files.
2. Every generated `pack.toml` must parse successfully via `Pack::load()` — the scaffold is a valid starting point.
3. An invalid pack name is always rejected before any file I/O.

### Required test scenarios

- [ ] `weave init <name>` creates all expected files with correct content
- [ ] `weave init <name>` rejects invalid names before creating any files
- [ ] `weave init <name>` fails cleanly when directory already exists
- [ ] `weave init` (no args) uses current directory name as pack name
- [ ] `weave init` fails when `pack.toml` already exists
- [ ] Generated `pack.toml` parses without error via `Pack::load()`

---

## 5. Environment Variable Handling

### Goal

When a pack declares `env` variables on its servers, each adapter writes only references (e.g. `$VAR_NAME`) into CLI config files — never values. Required env vars that are absent at install time produce a clear user-facing warning; optional absent vars are silent.

### Background

`McpServer.env` already exists in `src/core/pack.rs`:

```rust
pub struct EnvVar {
    pub required: bool,
    pub secret: bool,
    pub description: Option<String>,
}
```

This metadata drives both the reference format written to config and the warning logic at `apply()` time.

### Decision: warning on missing required, not error ✓

When `apply()` is called and a required env var is not set in the current environment, `apply()` writes the reference anyway and emits a warning. This is consistent with the collect-and-continue philosophy in `install.rs` — the intent is recorded, the user is told what to fix.

### Reference formats by CLI

These formats must be verified against each CLI's actual config schema before implementation:

| CLI | Config location | Reference format |
|-----|----------------|-----------------|
| Claude Code | `~/.claude.json` `mcpServers.<name>.env` | `"MY_TOKEN": "${MY_TOKEN}"` |
| Gemini CLI | `~/.gemini/settings.json` | `"MY_TOKEN": "$MY_TOKEN"` |
| Codex CLI | `~/.codex/config.toml` | `MY_TOKEN = "$MY_TOKEN"` |

> **Action required before implementation:** Confirm the exact env var reference format accepted by each CLI. The formats above are best-guess — a wrong format silently fails at AI CLI invocation time, which is hard to debug.

### Contracts

**apply() — secret env var writes reference, not value**

- Given a server with `env.MY_TOKEN = { required = true, secret = true }`
- And `MY_TOKEN=supersecret` is set in the current process environment
- When `apply()` is called
- Then the CLI config contains the reference format for that CLI (e.g. `"${MY_TOKEN}"`)
- And the string `supersecret` does not appear in any file weave writes

**apply() — missing required env var warns**

- Given `MY_TOKEN` is not set in the process environment
- And the server declares `env.MY_TOKEN = { required = true }`
- When `apply()` is called
- Then `apply()` returns `Ok(())`
- And a warning is printed: "Pack <name> requires env var MY_TOKEN — set it before using this pack" (or equivalent)
- And the reference `${MY_TOKEN}` is still written into the config

**apply() — missing optional env var is silent**

- Given `MY_OPTIONAL` is not set and declared as `{ required = false }`
- When `apply()` is called
- Then the reference is written to config
- And no warning is printed

**apply() — no env declared**

- Given a server with no `env` entries
- When `apply()` is called
- Then no `env` section is written to the CLI config for that server

### Invariants

1. The actual value of any env var is never written to any file by any adapter, regardless of `secret` flag.
2. `required = true` with var absent → warning. `required = false` with var absent → silent. Both write the reference.
3. Reference format is adapter-specific and lives exclusively in each adapter.

### Required test scenarios

- [ ] Secret env var set in environment: reference written, actual value absent from all output files
- [ ] Required env var missing: warning emitted, reference written, `Ok(())` returned
- [ ] Optional env var missing: no warning, reference written, `Ok(())` returned
- [ ] Server with no env: no env section in CLI config
- [ ] Reference format is correct for each adapter (separate test per adapter)

---

## 6. Improved Conflict Detection

### Goal

When two packs declare servers that export overlapping tool names, detect and surface the conflict at install time so the user can make an informed decision.

### Background

`McpServer.tools: Vec<String>` already exists in `src/core/pack.rs`. In M2 it is parsed but not used for conflict detection. This feature connects it to the install path.

### Conflict definition

A **tool conflict** occurs when:

1. Pack A declares server `S1` exporting tool `T`
2. Pack B (already installed) declares server `S2` also exporting tool `T`
3. `S1 != S2` (same tool name from two different servers — the AI CLI sees ambiguity)

Server-name conflicts (two packs trying to register a server with the same name) are already handled in M2 by the adapter's manifest check. This feature adds tool-level detection on top.

### Where conflict detection lives

Conflict detection does **not** belong in the `Resolver`. The resolver only calls `registry.fetch_metadata()`, which returns `PackMetadata` — this does not include server or tool lists (see `src/core/registry.rs:19-29`). Fetching full pack manifests from the registry for every installed pack at resolve time would be expensive and architecturally wrong.

**The correct location:** `cli/install.rs`, after loading the installed pack manifests from the local store but before calling adapters. Installed pack manifests are already on disk at `~/.packweave/packs/<name>/<version>/pack.toml` and can be loaded via `Pack::load()`.

If this logic grows beyond a few lines, extract it to a free function in `src/core/` (e.g. `conflict::check(incoming: &Pack, installed: &[Pack]) -> Vec<ToolConflict>`) — pure data-in, data-out, no I/O, unit-testable.

### Decision: warning + `--force`, not hard error ✓

Tool overlap is not always a problem. Install proceeds, but prints a list of conflicts. `weave install --force <pack>` suppresses the warning. Hard-blocking would prevent valid installs.

### Contracts

**install — no conflicts**

- Given no installed pack exports tool `T`
- When a new pack is installed that exports `T`
- Then install proceeds without any conflict warning

**install — tool conflict detected**

- Given pack "webdev" is installed and its server "puppeteer" exports tool `browser_navigate`
- When `weave install devtools` is run and "devtools" also has a server exporting `browser_navigate`
- Then install proceeds (not blocked)
- And output warns: "Tool conflict: browser_navigate is exported by both webdev/puppeteer and devtools/<server>"
- And exit code is 0

**install --force — conflict warning suppressed**

- Given the same conflict scenario
- When `weave install --force devtools` is run
- Then install proceeds without any conflict warning printed

**install — empty tools list**

- Given a server has `tools = []`
- When checking for conflicts
- Then no conflict is raised for that server (empty list means "tools unknown")

**install — server-name conflict is still adapter-handled**

- Given two packs register a server with the same name
- Then the adapter manifest check (M2 behavior) handles it — tool conflict detection does not duplicate this check

### Invariants

1. Conflict detection is purely informational — it never blocks install at this milestone.
2. Conflict detection only runs when **both** the incoming and installed server have non-empty `tools` lists.
3. Server-name conflicts remain handled by adapters; this feature handles only tool-name conflicts.
4. The conflict check function takes `&Pack` slices — no I/O, no registry calls, unit-testable in isolation.

### Required test scenarios

- [ ] No conflict: install proceeds silently
- [ ] Tool conflict: warning printed, install proceeds, exit 0
- [ ] `--force`: conflict warning suppressed
- [ ] Empty `tools` list on either side: no false-positive conflict
- [ ] Conflict check is a pure function (unit-testable without I/O)

---

## Cross-cutting requirements

### All M3 features

- All new public functions in `src/core/` must have unit tests in the same file.
- All new adapter code must have tests using `TempDir` — never real `~/.codex/` or `~/.claude/` paths.
- No new network calls in tests — extend `MockRegistry` as needed; mock HTTP for `--mcp` search.
- `cargo clippy -D warnings` and `cargo fmt --check` must pass.

### Spec maintenance

When a design decision is resolved, update this file: replace the `[DECISION]` block with the chosen option and a one-line rationale. Do not delete the rejected options — leave them struck-through so reviewers can see what was considered.

---

*Spec written at Milestone 2 merge. Implementation begins after the pre-implementation gate above is fully checked off.*
