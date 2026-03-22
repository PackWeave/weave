# Copilot instructions for weave

Use `AGENTS.md` as the canonical instruction source for this repository.

Before making code changes:

- Read `docs/ARCHITECTURE.md` and keep the documented module boundaries intact.
- Keep `src/cli/` handlers thin, keep business logic in `src/core/`, and keep CLI-specific config logic in `src/adapters/`.
- Follow Rust quality gates used in CI: `cargo fmt --all -- --check`, `cargo clippy -- -D warnings`, and `cargo test`.

When updating assistant guidance, edit `AGENTS.md` and keep `CLAUDE.md`, `GEMINI.md`, and `CODEX.md` as thin pointers.
