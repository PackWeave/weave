#!/bin/bash
# Pre-commit quality gate for weave.
# Intercepts `git commit` commands and runs cargo fmt, clippy, and test.
# Exit 2 blocks the commit and feeds the reason back to Claude.

INPUT=$(cat)
COMMAND=$(echo "$INPUT" | jq -r '.tool_input.command // empty')

FIRST_LINE=$(echo "$COMMAND" | head -1)
if echo "$FIRST_LINE" | grep -q 'git push'; then
    if echo "$FIRST_LINE" | grep -qE '(^|\s)(origin\s+)?(main|master)(\s|$)'; then
        if ! echo "$FIRST_LINE" | grep -qE '(--delete|-d)'; then
            echo "Blocked: direct push to main/master is not allowed." >&2
            echo "Create a feature branch, push it, and open a PR instead." >&2
            exit 2
        fi
    fi
fi

if echo "$COMMAND" | grep -q 'git commit'; then
    echo "Running pre-commit quality gate..." >&2

    if ! cargo fmt --all; then
        echo "cargo fmt failed — fix formatting before committing." >&2
        exit 2
    fi

    if ! cargo clippy -- -D warnings; then
        echo "cargo clippy failed — fix all warnings before committing." >&2
        exit 2
    fi

    if ! cargo test; then
        echo "cargo test failed — all tests must pass before committing." >&2
        exit 2
    fi

    echo "Quality gate passed." >&2
fi

exit 0
