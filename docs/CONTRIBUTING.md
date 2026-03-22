# Contributing

weave accepts two kinds of contributions: **code** and **packs**.

## AI assistant instructions

If you use AI assistants for contributions, read `AGENTS.md` at the repo root — it is the single source of truth for codebase conventions. `CLAUDE.md`, `GEMINI.md`, and `CODEX.md` are thin pointers to it.

If a PR was built with AI assistance, note the tool used in the PR description.

-----

## Contributing code

### Prerequisites

- Rust stable (latest)
- `cargo`, `clippy`, `rustfmt`

```bash
git clone https://github.com/PackWeave/weave
cd weave
cargo build
cargo test
```

## Running tests

```sh
cargo test
```

### Test structure

- **Unit tests** — `#[cfg(test)]` blocks in source files
- **Integration tests** — `tests/` directory (adapter tests, init tests)
- **E2E tests** — `tests/e2e/` (requires macOS/Linux, gated on Windows)

E2E tests use `wiremock` for mock HTTP, `assert_cmd` for subprocess assertions, and full isolation via environment variables (`HOME`, `WEAVE_TEST_STORE_DIR`, `WEAVE_REGISTRY_URL`).

### Before opening a PR

- `cargo fmt --all` — code must be formatted
- `cargo clippy -- -D warnings` — no clippy warnings
- `cargo test` — all tests must pass
- New behaviour must have tests
- Public types and functions must have doc comments

### Commit style

Use conventional commits:

```
feat: add gemini cli adapter
fix: remove orphaned command files on pack removal
docs: update adapter trait documentation
chore: bump clap to 4.5
```

### Opening issues

Use the issue templates in `.github/ISSUE_TEMPLATE/`. For bugs, include your OS, `weave --version`, and the exact command you ran.

-----

## Contributing packs

Packs are published to the `PackWeave/registry` repo via pull request.

### Pack creation quickstart (5 minutes)

1. Create a new folder and add a `pack.toml` with basic metadata and `[[servers]]` entries.
2. Add optional files under `prompts/`, `commands/`, or `settings/` as needed.
3. Run `weave init` once available to scaffold a pack, then validate with `weave publish`.

If you are not ready to build a pack, open a **Pack request** issue using the template in `.github/ISSUE_TEMPLATE/pack_request.md`.

### Pack quality bar

- Include a clear description and at least one keyword.
- Never include secrets or credential values.
- Prefer namespaces when tools may conflict.
- Declare tool lists when possible to enable conflict checks.
- Use CLI-specific prompts if behavior differs by CLI.

### Requirements for a pack to be accepted

- `pack.toml` is valid and passes `weave publish` validation
- All MCP servers referenced are publicly available
- Description is clear and accurate
- At least one tag
- No duplicate of an existing pack with the same purpose

### Process

1. Build your pack locally and test it with `weave install <local-path>`
1. Run `weave publish` — this creates the archive and opens a draft PR
1. A maintainer reviews and merges

### Pack naming

- Use lowercase and hyphens: `web-dev`, not `WebDev` or `web_dev`
- Be specific: `rust-embedded` not just `rust`
- Namespace with your username for personal packs: `@yourname/my-pack`

-----

## Questions

Open a GitHub Discussion, not an issue, for general questions.
