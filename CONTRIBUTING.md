# Contributing

PackWeave accepts two kinds of contributions: **code** and **packs**.

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
