# Contributing

Contributions are welcome — code improvements, bug fixes, and new packs all make weave better.

weave accepts two kinds of contributions: **code** and **packs**.

-----

## 💻 Contributing code

### 🛠️ Prerequisites

- Rust stable (latest release)
- `cargo`, `clippy`, `rustfmt`

```bash
git clone https://github.com/PackWeave/weave
cd weave
cargo build
./target/debug/weave --help    # confirm the binary built and runs
cargo test
```

> [!IMPORTANT]
> Always work on a feature branch — never commit directly to `main`. Use `feat/`, `fix/`, or `docs/` prefixes: `feat/my-feature`, `fix/bug-description`.

## 🧪 Running tests

```sh
cargo test
```

### 🏗️ Test structure

- **Unit tests** — `#[cfg(test)]` blocks in source files
- **Integration tests** — `tests/` directory (adapter tests, init tests)
- **E2E tests** — `tests/e2e/` (requires macOS/Linux, gated on Windows)

> [!NOTE]
> E2E tests require macOS or Linux — they are gated on Windows in CI. They use `wiremock` for mock HTTP, `assert_cmd` for subprocess assertions, and full isolation via environment variables (`HOME`, `WEAVE_TEST_STORE_DIR`, `WEAVE_REGISTRY_URL`).

### 🎨 CLI output style

All user-facing CLI output must use the style helpers in `src/cli/style.rs` (e.g. `style::pack_name()`, `style::version()`, `style::success()`). These respect `NO_COLOR`, `--color`, and TTY detection automatically. See `AGENTS.md` for the full list of semantic helpers.

### ✅ Before opening a PR

- `cargo fmt --all` — code must be formatted
- `cargo clippy -- -D warnings` — no clippy warnings
- `cargo test` — all tests must pass
- New behaviour must have tests
- Public types and functions must have doc comments
- CLI output uses `src/cli/style.rs` helpers — no raw ANSI codes

Once open, a maintainer will review within a few days. PRs addressing open issues are prioritised.

### 📝 Commit style

Use conventional commits:

```
feat: add gemini cli adapter
fix: remove orphaned command files on pack removal
docs: update adapter trait documentation
chore: bump clap to 4.5
```

### 💡 Proposing features

For non-trivial features, open an issue first to discuss scope and approach before writing code. Check [docs/ROADMAP.md](https://github.com/PackWeave/weave/blob/main/docs/ROADMAP.md) to see whether the feature is already planned or explicitly deferred.

### 🐛 Opening issues

Use the issue templates in `.github/ISSUE_TEMPLATE/`. For bugs, include your OS, `weave --version`, and the exact command you ran.

-----

## 📦 Contributing packs

Packs are published to the `PackWeave/registry` repo via pull request.

### ⚡ Pack creation quickstart (5 minutes)

1. Create a new folder and add a `pack.toml` with basic metadata and `[[servers]]` entries.
2. Add optional files under `prompts/`, `commands/`, or `settings/` as needed.
3. Run `weave init my-pack` to scaffold the directory structure.

> [!TIP]
> Not ready to build a pack yourself? Open a **Pack request** issue using the template in `.github/ISSUE_TEMPLATE/pack_request.md` and the community can pick it up.

### ✨ Pack quality bar

- Include a clear description and at least one keyword.
- Never include secrets or credential values.
- Prefer namespaces when tools may conflict.
- Declare tool lists when possible to enable conflict checks.
- Use CLI-specific prompts if behavior differs by CLI.

### ☑️ Requirements for a pack to be accepted

- `pack.toml` is valid and passes schema validation
- All MCP servers referenced are publicly available
- Description is clear and accurate
- At least one tag
- No duplicate of an existing pack with the same purpose

### 🔄 Process

1. Build your pack locally and test it with `weave install ./my-pack`
2. Open a pull request against [PackWeave/registry](https://github.com/PackWeave/registry) — add your pack source under `src/`
3. A maintainer reviews and merges; CI auto-generates `packs/{name}.json` from your `src/` entry

### 🏷️ Pack naming

- Use lowercase and hyphens: `web-dev`, not `WebDev` or `web_dev`
- Be specific: `rust-embedded` not just `rust`
- Namespace with your username for personal packs: `@yourname/my-pack`

-----

## 💬 Questions

Open a GitHub Discussion, not an issue, for general questions.

-----

## 🤖 AI assistant instructions

If you use AI assistants for contributions, read `AGENTS.md` at the repo root — it is the single source of truth for codebase conventions. `CLAUDE.md`, `GEMINI.md`, and `CODEX.md` are thin pointers to it.

If a PR was built with AI assistance, note the tool used in the PR description.
