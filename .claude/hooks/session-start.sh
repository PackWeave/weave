#!/bin/bash
# Session start orientation hook for weave.
# Prints a compact summary of the current work state to stderr
# so every session starts with full situational awareness.

BRANCH=$(git branch --show-current 2>/dev/null || echo "unknown")
DIRTY=$(git status --short 2>/dev/null | wc -l | tr -d ' ')
OPEN_PRS=$(gh pr list --state open --json number,title 2>/dev/null \
    | jq -r '.[].number' 2>/dev/null | tr '\n' '·' | sed 's/·$//' | sed 's/·/ · #/g')
OPEN_ISSUES=$(gh issue list --state open --json number 2>/dev/null \
    | jq 'length' 2>/dev/null || echo "?")

echo "━━━ weave — session start ━━━━━━━━━━━━━━━━━━━━━━━━" >&2
echo "  Branch : $BRANCH" >&2

if [ "$DIRTY" -gt 0 ] 2>/dev/null; then
    echo "  Status : $DIRTY uncommitted file(s)" >&2
else
    echo "  Status : clean" >&2
fi

if [ -n "$OPEN_PRS" ]; then
    echo "  PRs    : #$OPEN_PRS open" >&2
else
    echo "  PRs    : none open" >&2
fi

echo "  Issues : $OPEN_ISSUES open" >&2
echo "━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━" >&2

exit 0
