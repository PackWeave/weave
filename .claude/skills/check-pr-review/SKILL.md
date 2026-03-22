---
name: check-pr-review
description: Review all comments and reviews on a GitHub PR — inline code annotations, review verdicts (APPROVED / CHANGES_REQUESTED), and conversation threads. Classifies each as stale/valid/deferred/skip, fixes valid ones in the working tree, creates GitHub issues for deferred ones. Pass a PR number or omit to auto-detect from the current branch.
argument-hint: <PR number> (optional)
allowed-tools: Bash, Read, Edit, Write, Grep, Glob
---

## PR context

!`PR_ARG="$ARGUMENTS"; REPO=$(gh repo view --json nameWithOwner --jq .nameWithOwner 2>/dev/null); PR=${PR_ARG:-$(gh pr view --json number --jq .number 2>/dev/null)}; gh pr view "$PR" --json number,title,state,headRefName,additions,deletions --jq '"PR #\(.number): \(.title)\nState : \(.state)\nBranch: \(.headRefName)\nDiff  : +\(.additions) / -\(.deletions) lines"' 2>/dev/null || echo "ERROR: could not resolve PR — pass a PR number explicitly"`

## Inline code comments

!`PR_ARG="$ARGUMENTS"; REPO=$(gh repo view --json nameWithOwner --jq .nameWithOwner 2>/dev/null); PR=${PR_ARG:-$(gh pr view --json number --jq .number 2>/dev/null)}; gh api "repos/$REPO/pulls/$PR/comments" --paginate --jq 'sort_by(.path, (.line // .original_line)) | .[] | (if .in_reply_to_id then "  ↳ [reply \(.id)] [\(.user.login)]" else "[\(.id)] [\(.user.login)] \(.path):\(.line // .original_line // "?")" end) + "\n" + (if .in_reply_to_id then "    " else "  " end) + (.body | gsub("\n"; "\n  ")) + "\n"' 2>/dev/null || echo "(none)"`

## Review verdicts

!`PR_ARG="$ARGUMENTS"; REPO=$(gh repo view --json nameWithOwner --jq .nameWithOwner 2>/dev/null); PR=${PR_ARG:-$(gh pr view --json number --jq .number 2>/dev/null)}; gh api "repos/$REPO/pulls/$PR/reviews" --paginate --jq '.[] | "[\(.user.login) — \(.state)]" + (if .body != "" then "\n  " + (.body | gsub("\n"; "\n  ")) else " (no summary)" end) + "\n"' 2>/dev/null || echo "(none)"`

## Conversation comments

!`PR_ARG="$ARGUMENTS"; REPO=$(gh repo view --json nameWithOwner --jq .nameWithOwner 2>/dev/null); PR=${PR_ARG:-$(gh pr view --json number --jq .number 2>/dev/null)}; gh api "repos/$REPO/issues/$PR/comments" --paginate --jq 'if length == 0 then "(none)" else (.[] | "[\(.user.login)]\n  " + (.body | gsub("\n"; "\n  ")) + "\n") end' 2>/dev/null || echo "(none)"`

---

## Classification rules

For each **top-level** inline comment (entries without `↳`):

- **Stale** — the concern no longer applies to the current code (already fixed, file removed, logic changed). Do nothing.
- **Valid** — a real bug, security issue, or correctness problem not yet addressed. Fix it in the working tree now.
- **Deferred** — a reasonable suggestion (architecture, refactor, edge case handling) but out of scope for this PR. Create a GitHub issue.
- **Skip** — factually wrong, opinion-only with no actionable substance, or noise. Dismiss it.

For **reply threads** (`↳`): read the full thread for context before classifying the parent comment. A reply that resolves the concern makes the parent **stale**.

For **CHANGES_REQUESTED reviews**: treat each distinct concern in the review body as a comment to classify independently.

For **APPROVED reviews** and plain **COMMENTED** reviews with no code concerns: classify as **Skip** (acknowledge, no action).

## Steps

1. Read the injected data above
2. For each top-level inline comment:
   a. Read the referenced file at the referenced line (`path:line`) to see the current code state
   b. Read any reply thread beneath it for context
   c. Classify with a one-line reason
3. For each CHANGES_REQUESTED review body: extract distinct concerns and classify each
4. **Fix** all **valid** issues directly in the source files
5. After all fixes: `cargo clippy -- -D warnings && cargo test`
6. For each **deferred** issue: `gh issue create --title "..." --body "..."` — write a clear title and body explaining the concern and why it's worth tracking
7. Report the summary table:

| ID | Author | Location | Classification | Action |
|----|--------|----------|---------------|--------|
| `#id` | `login` | `path:line` | Stale / Valid / Deferred / Skip | Fixed in `path` / Issue #N / Dismissed |
