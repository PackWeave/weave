---
name: copilot-review
description: Review all Copilot inline comments on a PR. Classifies each comment as stale, valid, deferred, or skip — fixes valid ones in the working tree, creates GitHub issues for deferred ones. Pass the PR number as the argument.
allowed-tools: Bash, Read, Edit, Write, Grep
---

## PR #$ARGUMENTS — Copilot inline comments

!`gh api repos/$(gh repo view --json nameWithOwner --jq .nameWithOwner)/pulls/$ARGUMENTS/comments --jq '[.[] | select(.user.login | startswith("copilot")) | {path: .path, line: .original_line, body: .body}]'`

## Current branch

!`git branch --show-current`

## Classification rules

For each comment, assign one of:

- **Stale** — the code it references has already been fixed or the comment no longer applies to the current state of the PR/codebase. Do nothing.
- **Valid** — a real bug, security issue, or correctness concern that is not yet fixed. Fix it in the working tree now.
- **Deferred** — a reasonable suggestion (architecture, refactor, edge case) but out of scope or over-engineering for this PR. Create a GitHub issue.
- **Skip** — factually wrong, opinion-only, or no actionable substance. Dismiss it.

## Steps

1. Read the injected comment list above
2. For each comment, read the referenced file and line to understand the current code state
3. Classify the comment with a one-line reason
4. **Fix** all **valid** comments in the source files
5. After all fixes: run `cargo clippy -- -D warnings` and `cargo test` to confirm everything still compiles and passes
6. For each **deferred** comment: create a GitHub issue with `gh issue create --title "..." --body "..."` — write a clear title and body explaining the concern and why it's worth tracking. Do NOT create issues for stale or skipped comments.
7. Report a summary table:

| Comment | Classification | Action |
|---------|---------------|--------|
| `path:line — short description` | Stale / Valid / Deferred / Skip | Fixed / Issue #N / Dismissed |
