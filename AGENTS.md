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

### State safety

- Multi-step operations that mutate state (install, remove, profile switch) must validate preconditions **before** making any changes. If step 3 of 5 can fail, verify it will succeed before running steps 1 and 2.
- Never leave the user in a state that is neither the old nor the new. If a profile switch removes old packs and then fails to apply new ones, the user is stranded.
- When reviewing code with `continue` or `warn and skip` in a mutation loop, trace the full state: what files have been modified so far? Can the user recover?

### Mutating CLI config files

- **Read the docs/ARCHITECTURE.md section on non-destructive mutations before touching any adapter code**
- Every write must be tracked in the adapter's sidecar manifest
- Apply must be idempotent
- Remove must be surgical — leave user edits intact

### CLI output and color

All user-facing CLI output must use the style engine in `src/cli/style.rs`. Never emit raw ANSI escape codes or use hardcoded color strings. The style module respects `NO_COLOR`, `TERM=dumb`, non-TTY stdout, and the `--color` flag automatically.

Use the semantic helpers — not the color names:

- `style::pack_name("webdev")` — pack names (blue, bold)
- `style::version("1.2.3")` — version numbers (yellow, bold)
- `style::success("installed")` — success/ok messages (green, bold)
- `style::target("claude-code")` — CLI target names (teal)
- `style::dim("skipped")` — dimmed/secondary text (overlay)
- `style::subtext("description")` — descriptions (subtext)
- `style::header("Servers")` — section headers (mauve, bold)
- `style::emphasis("important")` — bold-only emphasis

Rules:

1. **All `println!`/`eprintln!` in `src/cli/`** must use `style::` helpers for structured data (pack names, versions, statuses, headers). Plain text messages are fine without styling.
2. **Never use `println!` in `src/core/` or `src/adapters/`** — return values or use `log::` macros.
3. **Error messages via `thiserror`** stay as plain text (they flow through `anyhow` at the CLI boundary, which formats them).
4. **New CLI commands or output changes** must be tested with `NO_COLOR=1` to verify graceful fallback.

### Testing

- Unit tests go in `#[cfg(test)]` blocks in the same file
- Integration tests go in `tests/` (adapter tests, init tests)
- E2E tests go in `tests/e2e/` — invoke the `weave` binary as a subprocess with full isolation via `TestEnv` (mock HTTP registry, fake HOME, fake store). Gated with `#[cfg(not(target_os = "windows"))]`
- Never write to the real `~/.claude/`, `~/.gemini/`, `~/.codex/` in tests — use `TempDir` and env var overrides (`WEAVE_TEST_STORE_DIR`, `WEAVE_REGISTRY_URL`, `HOME`)
- No network calls in tests — use `wiremock` for HTTP mocking
- Tests that mutate process-global env vars must use `#[serial]` from `serial_test` to prevent parallel races

### Naming

- `Pack` — a parsed, validated pack manifest
- `ResolvedPack` — a pack with its exact version pinned after resolution
- `InstalledPack` — a pack recorded in a profile
- `Profile` — a named collection of installed packs
- `Store` — the local pack cache
- `Registry` — the remote pack index (trait)
- `CompositeRegistry` — wraps official + tap registries with priority ordering
- `CliAdapter` — the trait for a specific CLI's config adapter
- `ApplyOptions` — options passed to adapter `apply()` (e.g. `allow_hooks` flag)
- `AdapterId` — stable machine identifier for adapters (ClaudeCode, GeminiCli, CodexCli)
- `HookEntry` — a parsed hook declaration from pack extensions
- `TapConfig` — a registered community tap in global config
- `Transport` — server transport type: `Stdio` or `Http`

-----

## Module map (quick reference)

```
src/cli/              Command handlers — parse args, call core, print output
  cli/install.rs        Thin wrapper → core::install
  cli/remove.rs         Pack removal
  cli/list.rs           List installed packs
  cli/search.rs         Pack + MCP registry search
  cli/update.rs         Thin wrapper → core::update
  cli/init.rs           Scaffold a new pack
  cli/profile.rs        Profile create/list/delete/add
  cli/use_profile.rs    Thin wrapper → core::use_profile
  cli/sync.rs           Reapply active profile
  cli/diagnose.rs       Config drift detection
  cli/tap.rs            Community tap add/list/remove
src/core/             Business logic — no I/O to CLI config files here
  core/config.rs        Global weave config (~/.packweave/config.toml)
  core/pack.rs          Pack manifest parsing + validation
  core/profile.rs       Profile read/write, active profile tracking
  core/lockfile.rs      Lock file read/write, version pinning
  core/resolver.rs      Dependency graph + semver resolution
  core/store.rs         Local pack cache (~/.packweave/packs/)
  core/registry.rs      Registry trait + GitHubRegistry + CompositeRegistry
  core/mcp_registry.rs  MCP Registry client (weave search --mcp)
  core/conflict.rs      Tool-level conflict detection
  core/install.rs       Install orchestration (registry + local)
  core/update.rs        Update orchestration (version comparison + apply)
  core/use_profile.rs   Profile switch orchestration (diff + remove + apply)
src/adapters/         CLI-specific config read/write — no business logic here
  adapters/mod.rs       CliAdapter trait + ApplyOptions
  adapters/claude_code.rs  Claude Code adapter (servers, commands, prompts, settings, hooks)
  adapters/gemini_cli.rs   Gemini CLI adapter (servers, prompts, settings)
  adapters/codex_cli.rs    Codex CLI adapter (servers, skills, prompts, settings)
src/error.rs          All error types
src/util.rs           Shared helpers (path resolution, file ops)
```

The CLI handlers are thin. They parse arguments, call into `core/` or `adapters/`, and format output. Business logic does not live in `cli/`.

The adapters are opaque. They expose only the `CliAdapter` trait. The core does not know about `settings.json` or `CLAUDE.md` — that's the adapter's concern.

-----

## Workflow skills

This project has Claude Code skills that encode the standard development workflow. Use them instead of doing these steps manually:

- **`/weave-ship <commit message>`** — full PR workflow: runs quality gates, commits, pushes, and opens a PR with the correct assignee. Use this whenever you are ready to ship a change.
- **`/rust-pre-commit`** — runs `cargo fmt --all`, `cargo clippy -- -D warnings`, and `cargo test` in order. Use this to verify the working tree is CI-ready before committing.
- **`/check-pr-review [PR number]`** — fetches all inline code comments, review verdicts (APPROVED/CHANGES_REQUESTED), and conversation threads on a PR. Classifies each as stale/valid/deferred/skip, fixes valid ones in-place, and creates GitHub issues for deferred ones. PR number is optional — auto-detected from the current branch.
- **`/weave-issue <title> [--- description]`** — creates a well-formed GitHub issue with current branch and recent commit context auto-injected. Use when deferring a finding or tracking follow-up work.
- **`/weave-e2e [flow]`** — runs the manual E2E validation checklist against real CLI installations (`~/.claude.json`, `~/.gemini/settings.json`, `~/.codex/config.toml`). This is the gate before shipping features that touch adapters. Run the full suite or target a single flow (`install`, `profiles`, `search`, `remove`, `diagnose`, `local`, `cleanup`).

Two hooks enforce workflow automatically:

- A `PreToolUse` hook runs the quality gate (`cargo fmt`, `cargo clippy`, `cargo test`) whenever Claude executes a `git commit` command.
- The same hook blocks any `git push` targeting `main` or `master` — all changes must go through a PR.
- A `SessionStart` hook prints branch, dirty file count, open PRs, and open issue count at the start of every session.

-----

## Git branch hygiene

**Never commit directly to `main`.** All changes must go through a pull request, even docs-only changes. Create a feature branch, push it, and open a PR via `gh pr create`.

**Always assign PRs to the current user.** When creating a PR, determine the GitHub username via `gh api user --jq .login` and pass it with `--assignee <username>`.

**Before committing to any branch, verify its PR has not already been merged into `main`.**

```sh
gh pr list --head <branch-name> --state merged
```

If the PR is merged, do not commit to that branch. Create a fresh branch from `main` instead. Committing to a merged branch creates orphaned history that must be untangled with cherry-picks.

**Always squash-merge PRs.** When merging, use `gh pr merge <number> --squash --delete-branch`. Never use `--merge` or `--rebase`. Release Please generates changelog entries from commits on main — squash ensures one clean entry per PR instead of noisy duplicates from every branch commit.

**Always commit and push changes immediately.** After editing a file, commit it to the appropriate branch and push before moving on to the next task. Never leave changes uncommitted in the working tree — they get lost on branch switches or session ends.

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

## Updating documentation

The project has two documents with module maps — they have different contracts:

- **`docs/ARCHITECTURE.md`** — aspirational design document. It explicitly lists modules that do not exist yet. **Never remove entries** just because the code has not been written. Only add new modules or update descriptions.
- **`AGENTS.md`** (this file) — quick reference for the current codebase. The module map here should match what actually exists in `src/`.

When your changes add new modules, CLI commands, or env vars, update both documents accordingly.

-----

## What not to do

- Do not write to `~/.claude/`, `~/.gemini/`, `~/.codex/`, or any real user config in tests
- Do not add dependencies without a clear reason — keep the dependency tree lean
- Do not put business logic in CLI handlers
- Do not put CLI-specific knowledge (file paths, config schemas) outside of adapters
- Do not use `println!` for output in library code — use `log` or return values
- Do not emit raw ANSI codes or hardcode colors — use `src/cli/style.rs` helpers
- Do not implement features listed under "Explicitly deferred" in docs/ROADMAP.md

-----

## Standards for "skip" decisions

Do not label issues as "cosmetic" or "low priority" to avoid fixing them. If something is wrong — a misleading comment, an inaccurate doc, a missing validation — fix it. The bar for skipping is:

1. The fix requires a design decision that needs human input
2. The fix is genuinely out of scope (belongs in a different PR or milestone)
3. The fix has no impact on correctness, maintainability, or future contributors

A misleading comment fails test 3 — it will mislead the next person who reads it. An inaccurate doc fails test 3 — it erodes trust. When in doubt, fix it now.
