---
name: weave-issue
description: Create a well-formed GitHub issue for the weave project with current work context auto-injected. Pass a title and optional description as arguments.
allowed-tools: Bash, Read, Grep, Glob
---

## Context

### Current branch
!`git branch --show-current`

### Recent commits
!`git log --oneline -5`

### Open issues
!`gh issue list --state open --json number,title --jq '.[] | "#\(.number) \(.title)"' 2>/dev/null | head -10`

## Task

Create a GitHub issue using the title and description from `$ARGUMENTS`.

Steps:
1. Parse `$ARGUMENTS`: the first line (or everything before `---`) is the **title**, anything after `---` is the **body/description**. If no separator, use the entire argument as the title.
2. Determine the GitHub username: `gh api user --jq .login`
3. Build a well-formed issue body that includes:
   - The description from `$ARGUMENTS` (if provided)
   - A **Context** section with: current branch, link to the most relevant recent commit if applicable
   - Keep it concise — no padding
4. Create the issue:
   ```sh
   gh issue create --title "<title>" --body "<body>" --assignee <username>
   ```
5. Print the issue URL.

Do not add labels unless the user specified them. Do not create duplicate issues — check the open issues list above first.
