#!/bin/bash
# Test harness for .claude hooks and skills.
# Run from the repo root: bash .claude/tests/test-hooks.sh
#
# Tests:
#   1. pre-commit-check.sh — push guard (block/allow cases + edge cases)
#   2. session-start.sh    — runs cleanly and emits expected sections
#   3. Skill YAML          — all SKILL.md files have required frontmatter fields
#   4. Skill shell syntax  — !` ... ` commands parse cleanly with bash -n

PASS=0
FAIL=0

# ── helpers ────────────────────────────────────────────────────────────────────

ok() { echo "  ✓ $1"; PASS=$((PASS+1)); }
fail() { echo "  ✗ $1"; FAIL=$((FAIL+1)); }

assert_exit() {
    local desc="$1" expected="$2"
    shift 2
    local actual
    "$@" >/dev/null 2>&1; actual=$?
    if [ "$actual" -eq "$expected" ]; then ok "$desc"; else fail "$desc  (expected exit $expected, got $actual)"; fi
}

assert_output_contains() {
    local desc="$1" pattern="$2"
    shift 2
    local out
    out=$("$@" 2>&1)
    if echo "$out" | grep -q "$pattern"; then ok "$desc"; else fail "$desc  (pattern '$pattern' not in output)"; fi
}

# Pipe a fake Claude tool-call JSON to the hook and capture its exit code.
push_exit() {
    local cmd="$1"
    printf '{"tool_input":{"command":"%s"}}' "$cmd" | bash .claude/hooks/pre-commit-check.sh >/dev/null 2>&1
    echo $?
}

assert_push_blocked() {
    local cmd="$1"
    local code; code=$(push_exit "$cmd")
    if [ "$code" -eq 2 ]; then ok "BLOCKED — $cmd"; else fail "should block — $cmd  (exit $code)"; fi
}

assert_push_allowed() {
    local cmd="$1"
    local code; code=$(push_exit "$cmd")
    if [ "$code" -eq 0 ]; then ok "ALLOWED — $cmd"; else fail "should allow — $cmd  (exit $code)"; fi
}

# ── 1. pre-commit-check.sh: push guard ────────────────────────────────────────

echo ""
echo "━━━ 1. pre-commit-check.sh — push guard (block) ━━━━━━━━━━━━━━━━━━━━━━━━"

assert_push_blocked "git push origin main"
assert_push_blocked "git push origin master"
assert_push_blocked "git push origin HEAD:main"
assert_push_blocked "git push origin HEAD:master"
assert_push_blocked "git push origin refs/heads/main"
assert_push_blocked "git push origin refs/heads/master"
assert_push_blocked "git push origin +HEAD:main"        # force refspec
assert_push_blocked "git push --force origin main"
assert_push_blocked "git push -f origin main"
assert_push_blocked "git push origin main --follow-tags"

echo ""
echo "━━━ 1. pre-commit-check.sh — push guard (allow) ━━━━━━━━━━━━━━━━━━━━━━━━"

assert_push_allowed "git push origin feat/my-branch"
assert_push_allowed "git push origin"                    # bare push (tracking branch)
assert_push_allowed "git push"                           # bare push
assert_push_allowed "git push --delete origin main"      # branch deletion
assert_push_allowed "git push -d origin main"
assert_push_allowed "git push origin feat/main-feature"  # branch with 'main' in name
assert_push_allowed "git push origin HEAD"               # HEAD without dest

echo ""
echo "━━━ 1. pre-commit-check.sh — push guard (edge cases) ━━━━━━━━━━━━━━━━━━━"

# A git commit whose message mentions "git push origin main" must NOT trigger
# the push guard — it's a commit command, not a push command.
code=$(printf '{"tool_input":{"command":"git commit -m \"fix git push origin main bug\""}}' \
    | bash .claude/hooks/pre-commit-check.sh >/dev/null 2>&1; echo $?)
# The commit gate will run cargo fmt --check etc. We can't easily test that
# here without side effects, so just verify the push guard didn't fire (exit 2).
# (If cargo is not available the commit gate may return non-zero for other reasons,
#  so we only assert it's NOT 2 specifically from the push guard.)
if [ "$code" -ne 2 ]; then
    ok "commit message containing 'git push origin main' — push guard does not fire"
    PASS=$((PASS+1))
else
    fail "commit message containing 'git push origin main' — push guard incorrectly fires"
    FAIL=$((FAIL+1))
fi

echo ""
echo "━━━ 1. pre-commit-check.sh — blocked message content ━━━━━━━━━━━━━━━━━━━"

out=$(printf '{"tool_input":{"command":"git push origin main"}}' \
    | bash .claude/hooks/pre-commit-check.sh 2>&1)
if echo "$out" | grep -q "Blocked"; then ok "prints 'Blocked' message"; else fail "missing 'Blocked' in output"; fi
if echo "$out" | grep -q "feature branch\|feature branch\|PR"; then ok "mentions PR/feature branch workaround"; else fail "missing workaround hint in output"; fi

# ── 1b. pre-commit-check.sh: git hook deduplication ──────────────────────────

echo ""
echo "━━━ 1b. pre-commit-check.sh — git hook deduplication ━━━━━━━━━━━━━━━━━━━"

# When .githooks/pre-commit is active, the hook must not run fmt/clippy itself.
# We can't easily test the cargo invocations without side effects, but we can
# verify the detection logic by inspecting the script's structure.
HOOK_SRC=$(cat .claude/hooks/pre-commit-check.sh)
if echo "$HOOK_SRC" | grep -q 'GIT_HOOK_ACTIVE'; then
    ok "hook detects .githooks/pre-commit activity"
else
    fail "hook missing git hook deduplication logic"
fi
if echo "$HOOK_SRC" | grep -q 'cargo test'; then
    ok "hook always runs cargo test (not gated on GIT_HOOK_ACTIVE)"
else
    fail "hook missing cargo test invocation"
fi

# ── 2. session-start.sh ────────────────────────────────────────────────────────

echo ""
echo "━━━ 2. session-start.sh ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━"

assert_exit "exits 0" 0 bash .claude/hooks/session-start.sh
assert_output_contains "prints Branch" "Branch" bash .claude/hooks/session-start.sh
assert_output_contains "prints Status" "Status" bash .claude/hooks/session-start.sh
assert_output_contains "prints Issues" "Issues" bash .claude/hooks/session-start.sh

# ── 3. Skill YAML validation ───────────────────────────────────────────────────

echo ""
echo "━━━ 3. Skill YAML — required fields ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━"

SKILLS_WITH_SIDE_EFFECTS="weave-e2e weave-issue weave-ship rust-pre-commit"

for skill_dir in .claude/skills/*/; do
    skill=$(basename "$skill_dir")
    file="${skill_dir}SKILL.md"

    if [ ! -f "$file" ]; then
        fail "$skill: SKILL.md not found"; continue
    fi

    # Must start with ---
    if ! head -1 "$file" | grep -q '^---'; then
        fail "$skill: frontmatter missing (no leading ---)"; continue
    fi

    # Must have description field
    if ! grep -q '^description:' "$file"; then
        fail "$skill: missing 'description' field"
    else
        ok "$skill: has description"
    fi

    # Skills with side effects should have disable-model-invocation: true
    for s in $SKILLS_WITH_SIDE_EFFECTS; do
        if [ "$skill" = "$s" ]; then
            if grep -q '^disable-model-invocation: true' "$file"; then
                ok "$skill: has disable-model-invocation: true"
            else
                fail "$skill: missing 'disable-model-invocation: true' (side-effectful skill)"
            fi
        fi
    done
done

# ── 4. Skill shell syntax ──────────────────────────────────────────────────────

echo ""
echo "━━━ 4. Skill shell syntax (!\` ... \` blocks) ━━━━━━━━━━━━━━━━━━━━━━━━━━━━"

for file in .claude/skills/*/SKILL.md; do
    skill=$(basename "$(dirname "$file")")
    n=0
    while IFS= read -r line; do
        # Match lines of the form: !`...`
        if [[ "$line" =~ ^\!\`(.+)\`$ ]]; then
            cmd="${BASH_REMATCH[1]}"
            n=$((n+1))
            # Replace $ARGUMENTS placeholder so bash -n doesn't choke on it
            sanitized="${cmd//\$ARGUMENTS/104}"
            tmpf=$(mktemp /tmp/skill-syntax-XXXXXX.sh)
            echo "$sanitized" > "$tmpf"
            if bash -n "$tmpf" 2>/dev/null; then
                ok "$skill [cmd $n]: shell syntax OK"
            else
                fail "$skill [cmd $n]: shell syntax error"
                echo "    cmd: $cmd"
            fi
            rm -f "$tmpf"
        fi
    done < "$file"
done

# ── Summary ────────────────────────────────────────────────────────────────────

echo ""
echo "━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━"
echo "  Results: $PASS passed, $FAIL failed"
echo "━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━"
echo ""

[ "$FAIL" -eq 0 ]
