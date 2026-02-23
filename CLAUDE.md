# weave — AI Assistant Instructions

This file is read by Claude and other AI assistants when working in this repository. It describes the codebase, conventions, and how to work effectively in it.

-----

## What this project is

weave is a Rust CLI tool that manages **packs** — versioned bundles of MCP server configurations, slash commands, and system prompts — across multiple AI CLIs (Claude Code, Gemini CLI, Codex CLI).

The core abstraction is the `CliAdapter` trait. All CLI-specific knowledge lives in adapters. The core never touches CLI config files directly.

Read ARCHITECTURE.md before writing any code. It defines the module structure, data flow, and key design decisions.

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

- **Read the ARCHITECTURE.md section on non-destructive mutations before touching any adapter code**
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
src/adapters/     CLI-specific config read/write — no business logic here
src/error.rs      All error types
src/config.rs     Global weave config
```

The CLI handlers are thin. They parse arguments, call into `core/` or `adapters/`, and format output. Business logic does not live in `cli/`.

The adapters are opaque. They expose only the `CliAdapter` trait. The core does not know about `settings.json` or `CLAUDE.md` — that's the adapter's concern.

-----

## What to do when asked to implement a feature

1. Check ROADMAP.md to confirm the feature is in scope and which milestone it belongs to
1. Read the relevant section of ARCHITECTURE.md
1. Write the types first — get the data model right before writing logic
1. Write tests before or alongside implementation
1. Run `cargo fmt`, `cargo clippy`, `cargo test` before considering it done

-----

## What not to do

- Do not write to `~/.claude/`, `~/.gemini/`, or any real user config in tests
- Do not add dependencies without a clear reason — keep the dependency tree lean
- Do not put business logic in CLI handlers
- Do not put CLI-specific knowledge (file paths, config schemas) outside of adapters
- Do not use `println!` for output in library code — use `log` or return values
- Do not implement features listed under "Explicitly deferred" in ROADMAP.md
