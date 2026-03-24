---
name: weave-e2e
description: Run the manual E2E validation checklist against real CLI installations on this machine. Tests weave against actual ~/.claude.json, ~/.gemini/settings.json, ~/.codex/config.toml — not mocks. Pass a flow name to run a targeted subset (install, profiles, search, remove, diagnose), or omit for the full suite.
allowed-tools: Bash, Read, Edit, Write, Glob, Grep
---

## Current state

### weave binary
!`weave --version 2>/dev/null || (cd "${CLAUDE_PROJECT_DIR:-.}" && cargo build --release 2>/dev/null && echo "Built. Add target/release to PATH or use ./target/release/weave") || echo "ERROR: weave not in PATH and build failed"`

### Installed CLIs
!`{ [ -d ~/.claude ] && echo "Claude Code: YES (~/.claude/)"; } 2>/dev/null; { [ -f ~/.gemini/settings.json ] && echo "Gemini CLI: YES (~/.gemini/settings.json)"; } 2>/dev/null; { [ -f ~/.codex/config.toml ] && echo "Codex CLI: YES (~/.codex/config.toml)"; } 2>/dev/null; true`

### Active profile
!`weave profile list 2>/dev/null || echo "weave not available"`

### Current ~/.claude.json MCP keys (baseline)
!`cat ~/.claude.json 2>/dev/null | jq '.mcpServers | keys' 2>/dev/null || echo "(no ~/.claude.json or no mcpServers key)"`

## Task

Run the E2E validation checklist from `.claude/skills/weave-e2e/checklist.md`.

**Arguments:** `$ARGUMENTS`

- If `$ARGUMENTS` is empty → run all flows in checklist order (Flows 1–17, then Flow 14: Cleanup last)
- If `$ARGUMENTS` is `install` → run Flows 1 + 2
- If `$ARGUMENTS` is `profiles` → run Flows 1 + 5
- If `$ARGUMENTS` is `search` → run Flows 1 + 7
- If `$ARGUMENTS` is `remove` → run Flows 1 + 6
- If `$ARGUMENTS` is `diagnose` → run Flows 1 + 3
- If `$ARGUMENTS` is `local` → run Flows 1 + 4
- If `$ARGUMENTS` is `project` → run Flows 1 + 8
- If `$ARGUMENTS` is `update` → run Flows 1 + 9
- If `$ARGUMENTS` is `tap` → run Flows 1 + 11
- If `$ARGUMENTS` is `hooks` → run Flows 1 + 12
- If `$ARGUMENTS` is `http` → run Flows 1 + 13
- If `$ARGUMENTS` is `auth` → run Flows 1 + 15
- If `$ARGUMENTS` is `init` → run Flows 1 + 10
- If `$ARGUMENTS` is `publish` → run Flows 1 + 16
- If `$ARGUMENTS` is `gap` → run Flows 1 + 17
- If `$ARGUMENTS` is `dry-run` → run Flows 1 + 18
- If `$ARGUMENTS` is `cleanup` → run Flow 14 only (safe to run any time)

## Steps

1. Read the checklist: `.claude/skills/weave-e2e/checklist.md`
2. Determine which flows to run based on arguments above
3. For each step in each flow:
   a. Print the step description
   b. Run the command
   c. Evaluate against expected output
   d. Mark ✓ (pass) or ✗ (fail) with a one-line explanation
4. If a flow fails a critical step, note it and continue to the next flow (don't abort the whole suite)
5. Always run Flow 14 (Cleanup) at the end of the full suite, even if earlier flows failed
6. Print the summary table from the checklist with actual ✓/✗ results filled in

## Important notes

- This modifies **real** config files (`~/.claude.json`, `~/.gemini/settings.json`, `~/.codex/config.toml`)
- The `e2e-validation` profile is used for isolation — always clean it up in Flow 14
- If the machine doesn't have a particular CLI installed, mark those CLI-specific steps as `N/A`
- If `weave` is not in PATH, look for it at `./target/release/weave` (build the project first)
- Do not abort on first failure — the goal is a complete picture of what works and what doesn't
