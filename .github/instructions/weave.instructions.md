---
applyTo: '**'
---

# GitHub Copilot Instructions — weave

weave is a Rust CLI tool that manages **packs** (MCP server configurations, prompts, commands, settings) across multiple AI CLIs. The central abstraction is the `CliAdapter` trait. All CLI-specific config knowledge lives in adapters — nowhere else.

**Read `docs/ARCHITECTURE.md` before reviewing any PR.** The module structure, data flow, and design decisions described there are intentional and must be preserved.

---

## Label every finding

Prefix every review comment with its category so findings can be triaged at a glance:

- `[correctness]` — logic error, wrong state transition, invariant violation
- `[security]` — path traversal, injection, unsafe deserialization, credential exposure
- `[architecture]` — module boundary breach
- `[robustness]` — missing error handling, silent failure, partial state left on disk
- `[low-priority]` — style, naming, minor improvements — note explicitly that this is low priority

Do not give `[low-priority]` findings the same visual weight as `[correctness]` or `[security]` findings. If a finding does not clearly fit categories 1–4, mark it `[low-priority]`.

---

## Review priority

1. `[correctness]` — logic errors, wrong assumptions, incorrect state transitions
2. `[security]` — path traversal, archive extraction safety, credential handling
3. `[architecture]` — module boundary breaches (see below)
4. `[robustness]` — missing error handling, silent failure, partial state
5. `[low-priority]` — style, naming, dead code, performance

---

## Architecture boundaries — never suggest violating

### `src/cli/` handlers are thin

- Parse arguments → call into `core/` or `adapters/` → format output
- No business logic, no direct file I/O
- Acceptable in handlers: `@`-prefix normalisation, version string parsing, output formatting
- Do not suggest moving logic into handlers

### `src/core/` is CLI-agnostic

- Never reads or writes any CLI config file (`settings.json`, `CLAUDE.md`, `.mcp.json`, etc.)
- Never imports from `src/adapters/`
- Interacts with adapters only through the `CliAdapter` trait
- Do not suggest adding CLI-specific knowledge to core modules

### `src/adapters/` are opaque

- All knowledge of a specific CLI's config format lives here and only here
- Adapters receive data (`ResolvedPack`) — they never query the registry or store themselves
- The data flow is fixed: **Registry → Resolver → Store → Profile + LockFile → Adapters**
- Do not suggest shortcutting this flow (e.g. a CLI handler calling an adapter directly)

---

## Core invariants — always verify when adapters are touched

### 1. Every write must be tracked in the manifest

The sidecar manifest (`.packweave_manifest.json`) tracks ownership of everything the adapter writes. A write that succeeds without a corresponding manifest update leaves an orphaned entry with no cleanup path.

**Always flag:** any code path where `util::write_file()` can succeed but the manifest map is not subsequently updated before `save_manifest()` is called.

### 2. `apply()` must be idempotent

Calling `apply()` twice must produce the same on-disk state as calling it once. Verify: servers (overwrite-safe), prompts (tagged blocks replaced, not doubled), commands (stale files purged on apply), settings (snapshot-based merge replaces existing fragment).

**Always flag:** any `apply()` path where a re-apply can duplicate entries.

### 3. `remove()` must be surgical

`remove()` consults the sidecar manifest to identify what it wrote. It must not delete entries it did not write, and must leave user-modified values in place with a warning rather than silently reverting them.

**Always flag:** any removal that deletes a key or entry by name without first confirming the manifest records this adapter as the owner.

### 4. Manifest records must survive errors in remove functions

In `remove_settings_from_file` and equivalent functions, ownership records must **not** be consumed from the in-memory map until after a successful write to disk. If the function returns an error (or an early `Ok(())`) between removing the record and writing the file, the record is permanently lost and future `remove()` / `diagnose()` calls cannot clean up correctly.

The correct pattern:

```rust
// CORRECT — peek without consuming; only remove after a successful write
let record = settings_map.get(pack_name).cloned()?;
// ... read file, modify config ...
util::write_file(path, &output)?;
settings_map.remove(pack_name); // only here, after success
```

**Always flag:** any `settings_map.remove(pack_name)` (or equivalent) that occurs before `util::write_file()`, where the function has error-return or early-return paths between the two calls.

### 5. `mcpServers` must be validated as an object before string indexing

`serde_json::Value`'s string-index operator (`value[&key] = ...`) **panics** if the value is not a JSON object. A user's hand-edited config file could contain `"mcpServers": []` or any non-object value. Before writing into `mcpServers`, always call `as_object_mut()` and return `ApplyFailed` if the result is `None`.

**Always flag:** any `servers_entry[&server_name] = ...` that is not preceded by an explicit `as_object_mut().ok_or_else(...)` guard on the same value.

### 6. Archive extraction must reject all traversal vectors

The store's `extract_archive` must check, in order, for every entry:

1. **Absolute paths** — both `Path::is_absolute()` AND a raw string prefix check for `/` and `\`. `is_absolute()` alone misses POSIX-style `/etc/evil` paths on Windows (no drive letter = not considered absolute).
2. **Parent directory components** — `path.components().any(|c| c == Component::ParentDir)`. Do not use `starts_with()` for this; `dest.join("../evil").starts_with(dest)` evaluates `true`.
3. **Symlinks and hardlinks** — `entry.header().entry_type().is_symlink() || is_hard_link()`. A symlink placed inside `dest` can point outside it; subsequent regular-file entries written through that symlink escape the destination directory without any `..` component.

**Always flag:** any modification to `extract_archive` that removes, weakens, or reorders these three checks.

### 7. Never use `with_extension()` on semver-versioned paths

Pack directories are named with full semver strings (e.g., `1.1.0`). `Path::with_extension("tmp")` treats the last `.`-delimited segment as the file extension, so `1.1.0` becomes `1.1.tmp` instead of `1.1.0.tmp`. This causes collisions between patch versions.

The correct pattern for appending a suffix to a semver directory name:

```rust
// CORRECT — appends to the full name as OsString
let tmp = dest.parent().expect("pack dir has a parent")
    .join({
        let mut name = dest.file_name().expect("pack dir has a file name").to_os_string();
        name.push(".tmp");
        name
    });

// WRONG — strips the patch version
let tmp = dest.with_extension("tmp"); // 1.1.0 → 1.1.tmp
```

**Always flag:** any use of `with_extension()` on a `PathBuf` whose final segment is a semver version string.

---

## Patterns known to be correct — do not flag

These were reviewed and intentionally accepted. Re-raising them is noise.

| Pattern | Why it is correct |
|---------|------------------|
| `mutex.lock().unwrap_or_else(\|e\| e.into_inner())` | Intentional mutex poison recovery — the inner value is still valid even if a previous holder panicked |
| `#[allow(dead_code)]` on `adapters`, `core`, `util` modules in `main.rs` | Intentional for Milestone 2 — stub code paths will be wired up in later milestones |
| `#[allow(dead_code)]` on `AlreadyInstalled`, `DependencyConflict`, `CliNotInstalled`, `RemoveFailed` in `error.rs` | Reserved variants, not yet reachable; defined for completeness |
| `expect("JSON serialization cannot fail")` on `serde_json::to_string_pretty` | Serialising a valid `serde_json::Value` cannot fail — invariant is correct, `expect` with comment is the right pattern |
| `expect("manifest serialization cannot fail")` on struct serialisation | Same — structs with `#[derive(Serialize)]` and only serialisable fields cannot fail |
| `content[start..].find(&end_tag)` in prompt remove/apply | **This is the correct anchored search.** It finds `end_tag` starting from `start`, ensuring `start..end` is always a valid range. Do not suggest reverting to an independent `content.find(&end_tag)`, which can produce invalid ranges when multiple blocks exist |
| Collect-and-continue in install/remove adapter loops | Intentional — the pack is always recorded in the profile/lockfile even when one adapter fails; errors surface as warnings; rollback is explicitly deferred |
| `pack_name.strip_prefix('@').unwrap_or(pack_name)` | Intentional UX normalisation — `@webdev` → `webdev` |
| Project-scope applied only when `.claude/` or `.gemini/` exists in cwd at install time | Intentional design decision — explicit over implicit; not a bug |

---

## Deferred items — do not re-flag

These are tracked in `docs/MILESTONE_2_FOLLOWUP.md`. Raising them as PR findings provides no value.

| Item | Reference |
|------|-----------|
| Cross-adapter transactional rollback on install/remove failure | FOLLOWUP.md, Deferred item 6 |
| Recursive (transitive) dependency resolution | `resolver.rs` comment; FOLLOWUP.md, Deferred item 3 |
| `reqwest::blocking::Client` with explicit timeouts | FOLLOWUP.md, Deferred item 2 |
| `lib.rs` split to scope `#[allow(dead_code)]` at item level | FOLLOWUP.md, Deferred item 5 |
| `weave search` using the weave registry rather than the upstream MCP Registry | FOLLOWUP.md, Deviation 4 |
| Windows support for Gemini CLI / Codex CLI adapters | ROADMAP.md, Explicitly deferred |
| Codex CLI adapter, `weave update`, `weave init`, env var handling | Milestone 3 — out of scope |
| Registry seeding, install script, Homebrew formula | Distribution artifacts outside this repo |

---

## Error handling conventions

- `thiserror` for error type definitions in library code (`src/core/`, `src/adapters/`, `src/error.rs`)
- `anyhow` with `.context(...)` for propagation in `src/cli/` handlers — never mix the two
- `unwrap()` / `expect()` in non-test code only with a comment explaining the invariant — flag if absent
- Silent `Ok(())` returned from a function that was supposed to mutate state is always `[robustness]`
- User-facing errors must state what went wrong **and** what the user can do about it — flag generic messages

---

## Testing requirements

**Always flag** a PR that:

- Adds or modifies adapter `apply()` / `remove()` logic without a test verifying the written file content and its restoration
- Uses a real `~/.claude/`, `~/.gemini/`, or `~/.packweave/` path in any test — must use `TempDir`
- Makes network calls in tests — the registry must always be mocked via `MockRegistry`
- Modifies `extract_archive` without a test that constructs a raw tar archive (using `make_raw_tar_gz` for path-manipulation tests) and verifies the rejection behaviour

Adapter tests must verify at minimum:
1. After `apply()`: the target config file contains exactly the expected entries
2. After `remove()`: the target config file is restored to its pre-apply state
3. `apply()` is idempotent: calling it twice produces the same on-disk state as calling it once

---

## What not to flag

- Style preferences not enforced by `rustfmt`
- Missing doc comments on private functions
- Suggestions to add dependencies without a clear correctness or security justification
- Anything already listed in `docs/MILESTONE_2_FOLLOWUP.md` under "Can be deferred"
- Structural differences between the Claude Code and Gemini CLI adapter implementations where the CLIs themselves differ (e.g., Claude Code has slash commands; Gemini does not — this is correct, not an inconsistency)
