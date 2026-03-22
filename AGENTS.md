# weave — AI Assistant Instructions

This is the canonical instruction file for all AI assistants working in this repository. `CLAUDE.md`, `GEMINI.md`, and `CODEX.md` are thin pointers to this file — always edit `AGENTS.md` only.

It describes the codebase, conventions, and how to work effectively in it.

-----

## What this project is

weave is a Rust CLI tool that manages **packs** — versioned bundles of MCP server configurations, slash commands, and system prompts — across multiple AI CLIs (Claude Code, Gemini CLI, Codex CLI).

The core abstraction is the `CliAdapter` trait. All CLI-specific knowledge lives in adapters. The core never touches CLI config files directly.

Read docs/ARCHITECTURE.md before writing any code. It defines the module structure, data flow, and key design decisions.

-----

## Language and toolchain

- **Rust stable**, latest version
- Format with `rustfmt` (default settings)
- Lint with `clippy` — treat all warnings as errors (`-D warnings`)
- No nightly features

-----

## Conventions

### Error handling

- Use `thiserror` for error type definitions in library code
- Use `anyhow` for propagation in CLI handler functions (`src/cli/`)
- Never use `unwrap()` or `expect()` in non-test code unless the condition is truly invariant — if you use one, add a comment explaining why it can't fail
- All user-facing errors must be actionable: say what went wrong AND what the user can do about it

### File operations

- All paths go through `dirs::home_dir()` — never hardcode `~` or `/home/user`
- Prefer `std::fs` for simple operations; `tokio::fs` only in async contexts
- Always check that a directory exists before writing into it

### Mutating CLI config files

- **Read the docs/ARCHITECTURE.md section on non-destructive mutations before touching any adapter code**
- Every write must be tracked in the adapter's sidecar manifest
- Apply must be idempotent
- Remove must be surgical — leave user edits intact

### Testing

- Unit tests go in `#[cfg(test)]` blocks in the same file
- Integration tests go in `tests/`
- Adapter tests use fixture directories under `tests/fixtures/` — never write to the real `~/.claude/` in tests
- No network calls in tests — mock the registry client

### Naming

- `Pack` — a parsed, validated pack manifest
- `ResolvedPack` — a pack with its exact version pinned after resolution
- `InstalledPack` — a pack recorded in a profile
- `Profile` — a named collection of installed packs
- `Store` — the local pack cache
- `Registry` — the remote pack index (trait)
- `CliAdapter` — the trait for a specific CLI's config adapter

-----

## Module map (quick reference)

```
src/cli/          Command handlers — parse args, call core, print output
src/core/         Business logic — no I/O to CLI config files here
  core/mcp_registry.rs   Upstream MCP registry integration
  core/conflict.rs       Tool-level conflict detection across installed packs
src/adapters/     CLI-specific config read/write — no business logic here
src/error.rs      All error types
src/core/config.rs    Global weave config
src/util.rs       Shared helpers (file ops, path resolution, etc.)
```

The CLI handlers are thin. They parse arguments, call into `core/` or `adapters/`, and format output. Business logic does not live in `cli/`.

The adapters are opaque. They expose only the `CliAdapter` trait. The core does not know about `settings.json` or `CLAUDE.md` — that's the adapter's concern.

-----

## Git branch hygiene

**Never commit directly to `main`.** All changes must go through a pull request, even docs-only changes. Create a feature branch, push it, and open a PR via `gh pr create`.

**Before committing to any branch, verify its PR has not already been merged into `main`.**

```sh
gh pr list --head <branch-name> --state merged
```

If the PR is merged, do not commit to that branch. Create a fresh branch from `main` instead. Committing to a merged branch creates orphaned history that must be untangled with cherry-picks.

-----

## What to do when asked to implement a feature

1. Check docs/ROADMAP.md to confirm the feature is in scope and which milestone it belongs to
1. Read the relevant section of docs/ARCHITECTURE.md
1. Write the types first — get the data model right before writing logic
1. Write tests before or alongside implementation
1. Run `cargo fmt --all`, `cargo clippy -- -D warnings`, `cargo test` before considering it done — **in that order, every time**

-----

## Adding or modifying a CLI adapter

**Before writing any adapter code, verify the target CLI's actual config format from primary sources.** Do not rely on prior knowledge, architecture docs, or assumptions — CLI tools change their config schemas. A wrong assumption (e.g. JSON vs TOML, missing MCP support) means a fundamentally broken adapter that must be fully rewritten.

Mandatory research checklist before touching `src/adapters/`:

1. **Find the official repo** — read the README and any `config.*` or `schema.*` files in the source tree
2. **Confirm the config file path and format** — exact filename, location (`~/.foo/` vs `~/.config/foo/`), serialization format (TOML / JSON / YAML)
3. **Confirm MCP server support** — how are servers declared? What fields does each entry require?
4. **Confirm project-scope config** — is there a `.foo/` project directory? What does it contain?
5. **Confirm the prompt/instruction file** — exact filename and how it is discovered (single file vs hierarchical walk)
6. **Confirm slash commands / skills equivalent** — does the CLI have a user-scriptable command directory?
7. **Cross-check docs/ARCHITECTURE.md** — if what you find contradicts the architecture doc, update the architecture doc to match reality before writing code

Record your findings in a comment at the top of the adapter file. If the CLI does not support a feature (e.g. MCP servers), say so explicitly in a comment so the next person does not re-research it.

-----

## Local pre-commit hook

The repo ships a pre-commit hook that mirrors CI. Activate it once after cloning:

```sh
git config core.hooksPath .githooks
```

After activation, every `git commit` runs `cargo fmt --check` and `cargo clippy` locally. This catches formatting and lint failures before push rather than in CI.

-----

## What not to do

- Do not write to `~/.claude/`, `~/.gemini/`, `~/.codex/`, or any real user config in tests
- Do not add dependencies without a clear reason — keep the dependency tree lean
- Do not put business logic in CLI handlers
- Do not put CLI-specific knowledge (file paths, config schemas) outside of adapters
- Do not use `println!` for output in library code — use `log` or return values
- Do not implement features listed under "Explicitly deferred" in docs/ROADMAP.md
