---
name: review-prs
description: Run multi-persona code review (PO, Senior Eng, Security, Docs) on open PRs. Pass PR numbers or omit to auto-detect from recent branches. Runs 2 rounds by default.
allowed-tools: Bash, Read, Grep, Glob, Agent
argument-hint: "[PR numbers] [--rounds N]"
---

## Context

### Open PRs by current user
!`gh pr list --author @me --state open --json number,title,headRefName --jq '.[] | "#\(.number) \(.title) [\(.headRefName)]"' 2>/dev/null | head -10`

### Repository
!`gh repo view --json nameWithOwner --jq .nameWithOwner 2>/dev/null`

## Task

Run a multi-persona code review on the specified PRs. Each persona reviews ALL PRs independently, then a second round digs deeper based on round 1 findings.

**Arguments:** `$ARGUMENTS`

- If PR numbers are provided (e.g., `233 234 235`), review those PRs
- If no arguments, review all open PRs by the current user (from context above)
- `--rounds N` sets the number of review rounds (default: 2)

## Personas

### 1. Product Owner
Focus: user experience, error messages, edge cases users will hit, feature completeness, output quality.
- Are error messages clear, actionable, and user-friendly?
- What happens when things go wrong in unexpected ways?
- Does the feature behave as a user would expect?
- Does it deliver what the issue promised?

### 2. Senior Rust Engineer
Focus: code quality, idiomatic Rust, error handling, performance, architecture, test quality.
- Proper use of Result, Option, lifetimes, ownership?
- WeaveError vs anyhow usage correct per layer (core vs cli)?
- Unnecessary allocations, clones, or I/O?
- Does it follow the cli → core → adapters layered architecture?
- Are tests thorough? Do they test failure paths?
- Any dead code or unused imports?

### 3. Security Auditor
Focus: attack surface, TOCTOU, path traversal, injection, lock bypass, supply chain.
- Can any check be bypassed?
- Time-of-check-time-of-use gaps?
- Error message information leakage?
- New dependency supply chain risk?
- Could a malicious pack or concurrent process exploit the change?

### 4. Documentation Specialist
Focus: doc comments, help text, error message consistency, docs/ARCHITECTURE.md and AGENTS.md updates.
- Are new public functions documented?
- Does ARCHITECTURE.md need updating? (new modules, state files, error types)
- Does AGENTS.md module map need updating?
- Are error messages consistent with project style ("what went wrong + what to do")?

## Steps

### Round 1: Initial review

Launch all 4 personas in parallel as background agents. Each agent:
1. Reads the diff and changed files for every PR being reviewed
2. Evaluates from their persona's perspective
3. Reports findings per PR with severity (Critical / High / Medium / Low) and specific file:line references

**Agent prompt template for each persona:**
```
You are a [PERSONA] reviewing [N] PRs for the weave project.
[PERSONA FOCUS DESCRIPTION FROM ABOVE]

Review these PRs by reading the actual code changes:
[LIST EACH PR: number, branch, worktree path or files changed]

For each PR, read the changed files and evaluate from your perspective.
Report findings per PR with severity (Critical/High/Medium/Low) and specific line numbers.
```

To find files for each PR, use:
```
gh pr diff [NUMBER] --name-only
```

And read them from the PR branch (check it out or use worktree paths if available).

### Compile round 1

After all 4 agents complete, compile findings into a single table:

| PR | Persona | Severity | Finding | File:Line |
|----|---------|----------|---------|-----------|

### Round 2: Deep dive

Launch 4 new persona agents that receive the round 1 findings as context. Each agent:
1. Re-reads the code with round 1 findings in mind
2. Verifies or challenges other personas' findings
3. Looks for issues that round 1 missed, specifically:
   - **PO**: trace the full user journey for each error path
   - **Eng**: trace every code path for correctness, check for race conditions
   - **Security**: attempt to construct exploit scenarios for each finding
   - **Docs**: verify every public API change has documentation

**Round 2 agent prompt addition:**
```
Round 1 found these issues:
[PASTE COMPILED TABLE]

Your job in round 2:
1. Verify or challenge these findings — are they real or false positives?
2. Look deeper at the areas round 1 flagged
3. Find issues that round 1 missed
```

### Final report

After round 2 completes, produce the consolidated report:

1. **Summary table** with all unique findings, deduplicated, with final severity
2. **Action items** — which findings need fixes before merge vs. which are acceptable
3. **Verdict per PR** — Ready to merge / Needs fixes / Needs redesign
