---
name: rust-pre-commit
description: Run the Weave quality gate (cargo fmt, clippy, tests) before committing. Use when you want to verify the working tree is clean and CI-ready.
allowed-tools: Bash, Read, Grep, Glob
---

## Weave pre-commit quality gate

Run the three checks in order. Stop and report on the first failure — do not proceed to the next step if one fails.

1. `cargo fmt --all` — formats all code in place; then run `git diff --name-only` to list any files that were changed and need to be staged before committing
2. `cargo clippy -- -D warnings` — lint, deny all warnings
3. `cargo test` — run all unit + integration + E2E tests

Report clearly:
- Which checks passed ✓
- Which failed ✗ and what output was produced
- What the developer needs to fix before committing
