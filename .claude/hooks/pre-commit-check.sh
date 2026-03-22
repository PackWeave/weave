#!/bin/bash
# Pre-commit quality gate for weave.
# Intercepts `git commit` commands and runs cargo fmt, clippy, and test.
# Exit 2 blocks the commit and feeds the reason back to Claude.

INPUT=$(cat)
COMMAND=$(echo "$INPUT" | jq -r '.tool_input.command // empty')

FIRST_LINE=$(echo "$COMMAND" | head -1)
if echo "$FIRST_LINE" | grep -qE '^[[:space:]]*git[[:space:]]+push\b'; then
    # Parse args after 'git push' to catch refspec forms: origin main, HEAD:main, refs/heads/main
    PUSH_ARGS=$(echo "$FIRST_LINE" | sed -E 's/^[[:space:]]*git[[:space:]]+push[[:space:]]*//')
    is_delete=false
    dest_to_main=false
    # shellcheck disable=SC2086
    set -- $PUSH_ARGS
    for arg in "$@"; do
        case "$arg" in
            --delete|-d) is_delete=true ;;
            --*|-*) ;;  # skip other flags
            *)
                # Could be remote name or refspec (src:dest or bare dest)
                refspec="${arg#+}"
                dest="$refspec"
                case "$refspec" in *:*) dest="${refspec##*:}" ;; esac
                short_dest="${dest#refs/heads/}"
                if [ "$short_dest" = "main" ] || [ "$short_dest" = "master" ]; then
                    dest_to_main=true
                fi
                ;;
        esac
    done
    if [ "$dest_to_main" = true ] && [ "$is_delete" = false ]; then
        echo "Blocked: direct push to main/master is not allowed." >&2
        echo "Create a feature branch, push it, and open a PR instead." >&2
        exit 2
    fi
fi

if echo "$COMMAND" | grep -q 'git commit'; then
    echo "Running pre-commit quality gate..." >&2

    # Check if .githooks/pre-commit is active — if so, skip fmt/clippy here to
    # avoid running them twice (git will fire the hook itself during commit).
    HOOKS_PATH=$(git config core.hooksPath 2>/dev/null)
    GIT_HOOK_ACTIVE=false
    if [ "$HOOKS_PATH" = ".githooks" ] && [ -x ".githooks/pre-commit" ]; then
        GIT_HOOK_ACTIVE=true
    fi

    if [ "$GIT_HOOK_ACTIVE" = false ]; then
        if ! cargo fmt --all -- --check; then
            echo "cargo fmt check failed — run 'cargo fmt --all' locally, restage the changes, and retry." >&2
            exit 2
        fi

        if ! cargo clippy -- -D warnings; then
            echo "cargo clippy failed — fix all warnings before committing." >&2
            exit 2
        fi
    fi

    # Always run tests — .githooks/pre-commit intentionally skips them for speed.
    if ! cargo test; then
        echo "cargo test failed — all tests must pass before committing." >&2
        exit 2
    fi

    echo "Quality gate passed." >&2
fi

exit 0
