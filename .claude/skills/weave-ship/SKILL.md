---
name: weave-ship
description: Full Weave workflow from working changes to open PR. Runs quality gates, commits, pushes, and opens a PR with the correct assignee. Use when ready to ship a change. Pass the commit message as the argument.
allowed-tools: Bash, Read, Edit, Write, Grep, Glob
---

## Current state

- Branch: !`git branch --show-current`
- Uncommitted changes: !`git status --short`
- GitHub user: !`gh api user --jq .login 2>/dev/null || echo "(not authenticated — run gh auth login)"`

## Commit message / PR title

$ARGUMENTS

## Weave ship workflow

### Rules (non-negotiable)

- Never commit directly to `main` — if on main, stop immediately and tell the user to create a feature branch
- Run quality gates in order; stop on first failure
- Before committing, verify the branch PR hasn't already been merged: `gh pr list --head <branch> --state merged`
- Always assign the PR to the current GitHub user (`--assignee <user>`)
- No "Built with Claude Code" or similar attribution taglines in commit messages, PR titles, or PR bodies

### Steps

1. **Check branch** — if current branch is `main`, stop and tell the user to create a feature branch first

2. **Check PR not already merged** — run `gh pr list --head $(git branch --show-current) --state merged`; if any result, stop and warn that the branch was already merged

3. **Pre-commit** — run these before committing (the git pre-commit hook handles `fmt --check` and `clippy`, so don't duplicate those):
   - `cargo fmt --all` — auto-fix formatting so the hook's `--check` passes
   - `cargo test` — the hook does not run tests, so run them here; stop on failure

4. **Commit** — if $ARGUMENTS is empty, ask the user for a commit message before proceeding; otherwise use $ARGUMENTS as the commit message. Stage all modified tracked files and new relevant files, then commit. No attribution taglines.

5. **Push** — `git push -u origin $(git branch --show-current)`

6. **Open PR** — `gh pr create --title "<title>" --body "<body>" --assignee <github-user>`
   - Derive the title from the commit message
   - Write a concise PR body (Summary + Test plan sections)
   - No attribution taglines anywhere

7. **Report** — print the PR URL
