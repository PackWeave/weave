---
name: weave-issue
description: Create a well-formed GitHub issue for the weave project with all fields populated (labels, type, milestone, assignee, blocked-by relationships). Pass a title and optional description as arguments.
allowed-tools: Bash, Read, Grep, Glob
argument-hint: "<title> [--- <description>]"
---

## Context

### Current branch
!`git branch --show-current`

### Recent commits
!`git log --oneline -5`

### Open issues (for dedup check)
!`gh issue list --state open --json number,title --jq '.[] | "#\(.number) \(.title)"' 2>/dev/null | head -20`

### Available milestones
!`gh api repos/PackWeave/weave/milestones --jq '.[] | "\(.title)"' 2>/dev/null`

### Issue type IDs (for GraphQL)
!`gh api graphql -f query='{ repository(owner: "PackWeave", name: "weave") { issueTypes(first: 10) { nodes { id name } } } }' --jq '.data.repository.issueTypes.nodes[] | "\(.name) = \(.id)"' 2>/dev/null`

## Task

Create a GitHub issue with **all fields filled**: label, issue type, milestone, assignee, and blocked-by relationships.

### Step 1 — Parse arguments

Parse `$ARGUMENTS`:
- Everything before `---` (or the entire input if no separator) is the **title**
- Everything after `---` is the **body/description**
- If no arguments, ask the user for a title

### Step 2 — Classify the issue

Based on the title prefix and content, determine:

**Issue type** (exactly one):
- `Bug` — title starts with `fix:` or `fix(`, or describes something broken
- `Feature` — title starts with `feat:` or `feat(`, or describes new functionality
- `Task` — title starts with `test:`, `ci:`, `docs:`, `refactor:`, `chore:`, or describes internal work

**Label** (exactly one):
- `bug` — for Bug type
- `enhancement` — for Feature and Task types
- `documentation` — for docs-only issues

### Step 3 — Determine milestone

Ask the user which milestone this belongs to, showing the available milestones from context above. If the user has already specified a milestone in their description, use that. Options:
- `M6 - 0.5` (security hardening)
- `M7 - 0.6` (ecosystem depth)
- `M8 - 0.7` (power features)
- None (explicitly deferred)

### Step 4 — Check for blockers

Ask: "Does this issue depend on any open issue being completed first?"
- If yes, collect the blocking issue numbers
- If the user says no or doesn't specify, skip this step

### Step 5 — Create the issue

```sh
gh issue create \
  --title "<title>" \
  --body "<body>" \
  --assignee $(gh api user --jq .login) \
  --label "<label>" \
  --milestone "<milestone>"  # omit flag entirely if no milestone
```

Do not create duplicate issues — check the open issues list above first. No "Built with Claude Code" taglines.

### Step 6 — Set issue type

Get the new issue's node ID and set the type **in a single Bash call** (variables don't persist across calls):

```sh
NODE_ID=$(gh api graphql -f query='{ repository(owner: "PackWeave", name: "weave") { issue(number: <NUMBER>) { id } } }' --jq '.data.repository.issue.id') && \
gh api graphql -f query='mutation { updateIssue(input: {id: "'$NODE_ID'", issueTypeId: "<TYPE_ID>"}) { issue { number } } }'
```

### Step 7 — Set blocked-by relationships

If blocking issues were identified in Step 4, for each blocker run **in a single Bash call** (NODE_ID from Step 6 won't persist — re-fetch it, or chain all steps together):

```sh
NODE_ID=$(gh api graphql -f query='{ repository(owner: "PackWeave", name: "weave") { issue(number: <NUMBER>) { id } } }' --jq '.data.repository.issue.id') && \
BLOCKER_ID=$(gh api graphql -f query='{ repository(owner: "PackWeave", name: "weave") { issue(number: <BLOCKER_NUMBER>) { id } } }' --jq '.data.repository.issue.id') && \
gh api graphql -f query='mutation { addBlockedBy(input: {issueId: "'$NODE_ID'", blockingIssueId: "'$BLOCKER_ID'"}) { issue { number } blockingIssue { number } } }' && \
gh issue edit <NUMBER> --add-label "blocked"
```

Repeat for each additional blocker, or loop:
```sh
for BLOCKER_NUM in <NUM1> <NUM2>; do
  BLOCKER_ID=$(gh api graphql -f query='{ repository(owner: "PackWeave", name: "weave") { issue(number: '$BLOCKER_NUM') { id } } }' --jq '.data.repository.issue.id') && \
  gh api graphql -f query='mutation { addBlockedBy(input: {issueId: "'$NODE_ID'", blockingIssueId: "'$BLOCKER_ID'"}) { issue { number } blockingIssue { number } } }'
done
gh issue edit <NUMBER> --add-label "blocked"
```

### Step 8 — Report

Print a summary:
```
Created: #<number> <title>
Type:    <Bug|Feature|Task>
Label:   <label>
Milestone: <milestone>
Blocked by: #X, #Y (if any)
URL:     <url>
```
