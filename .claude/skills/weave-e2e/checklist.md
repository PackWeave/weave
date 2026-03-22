# weave E2E Validation Checklist

This checklist tests weave against **real CLI installations** on the current machine.
It is NOT a replacement for automated tests — it catches adapter schema drift and
config format mismatches that mock-based tests cannot detect.

Run via `/weave-e2e` or `/weave-e2e <flow>` to target a specific flow.

---

## Flow 1: Environment check

**Goal:** Confirm weave binary and detect which CLIs are installed.

| Step | Command | Expected |
|------|---------|----------|
| 1.1 | `weave --version` | Prints a version string (e.g. `weave 0.5.0`) |
| 1.2 | `ls ~/.claude/ 2>/dev/null && echo "Claude Code: YES" \|\| echo "Claude Code: NO"` | Reports presence |
| 1.3 | `ls ~/.gemini/ 2>/dev/null && echo "Gemini CLI: YES" \|\| echo "Gemini CLI: NO"` | Reports presence |
| 1.4 | `ls ~/.codex/ 2>/dev/null && echo "Codex CLI: YES" \|\| echo "Codex CLI: NO"` | Reports presence |
| 1.5 | `weave profile list` | Prints at least `default` profile |

**Pass criteria:** weave --version succeeds and at least one CLI is detected.

---

## Flow 2: Registry install

**Goal:** Install a pack from the registry and verify config file mutations.

**Pack to use:** `filesystem` (universally available in registry)

| Step | Command | Expected |
|------|---------|----------|
| 2.1 | Snapshot: `cat ~/.claude.json 2>/dev/null \| jq '.mcpServers \| keys' 2>/dev/null` | Baseline MCP keys |
| 2.2 | `weave install filesystem` | Exits 0, prints "Installed filesystem" |
| 2.3 | `cat ~/.claude.json \| jq '.mcpServers \| keys'` | Contains filesystem server key(s) |
| 2.4 | `weave list` | Shows `filesystem` |
| 2.5 | (If Gemini installed) `cat ~/.gemini/settings.json \| jq '.mcpServers \| keys'` | Contains filesystem |
| 2.6 | (If Codex installed) `cat ~/.codex/config.toml` | Contains filesystem entry |

**Pass criteria:** All installed CLIs show filesystem in their config.

---

## Flow 3: Diagnose

**Goal:** Verify `weave diagnose` accurately reflects installed state.

| Step | Command | Expected |
|------|---------|----------|
| 3.1 | `weave diagnose` | Exits 0; no `Missing` entries for `filesystem` |
| 3.2 | `weave diagnose --json` | Valid JSON; `"name": "filesystem"` present with no errors |
| 3.3 | `weave diagnose --json \| jq 'keys'` | Top-level keys include `packs` or `results` |

**Pass criteria:** `weave diagnose` exits 0 and reports filesystem as healthy.

---

## Flow 4: Local pack install

**Goal:** Install a pack from a local directory path.

**Setup:** Create a minimal pack at `/tmp/weave-e2e-local/`

```toml
# /tmp/weave-e2e-local/pack.toml
[pack]
name = "e2e-local-test"
version = "0.1.0"
description = "E2E local pack test"
authors = ["e2e-tester"]
```

| Step | Command | Expected |
|------|---------|----------|
| 4.1 | Create pack dir + `pack.toml` as above | Files exist |
| 4.2 | `weave install /tmp/weave-e2e-local` | Exits 0, prints "Installed e2e-local-test@0.1.0 (local)" |
| 4.3 | `weave list` | Shows `e2e-local-test` |
| 4.4 | Re-run `weave install /tmp/weave-e2e-local` | Exits 0 (idempotent, no duplicate warning) |

**Pass criteria:** Local install succeeds and shows in list.

---

## Flow 5: Profiles

**Goal:** Verify profile create, add, use, and sync.

| Step | Command | Expected |
|------|---------|----------|
| 5.1 | `weave profile create e2e-validation` | Exits 0 |
| 5.2 | `weave profile add filesystem -p e2e-validation` | Exits 0 |
| 5.3 | `weave use e2e-validation` | Exits 0; prints switch confirmation |
| 5.4 | `weave list` | Shows `filesystem` under active profile |
| 5.5 | `weave sync` | Exits 0, no errors |
| 5.6 | `cat ~/.claude.json \| jq '.mcpServers \| keys'` | filesystem still present |

**Pass criteria:** Profile switch applies correctly; sync is a no-op when already in sync.

---

## Flow 6: Non-destructive remove

**Goal:** Verify `weave remove` leaves user config intact (sentinel comment test).

| Step | Command | Expected |
|------|---------|----------|
| 6.1 | Note filesystem keys before removal: `weave list` | Baseline |
| 6.2 | `weave remove filesystem` | Exits 0 |
| 6.3 | `cat ~/.claude.json \| jq '.mcpServers \| keys'` | filesystem keys absent |
| 6.4 | `weave list` | filesystem no longer listed |
| 6.5 | Check other mcpServers keys unchanged | No regressions in user config |

**Pass criteria:** filesystem removed cleanly; no other config sections modified.

---

## Flow 7: Search

**Goal:** Verify registry and MCP search work.

| Step | Command | Expected |
|------|---------|----------|
| 7.1 | `weave search filesystem` | Returns results from weave registry |
| 7.2 | `weave search --mcp filesystem` | Contacts MCP Registry; returns results |
| 7.3 | `weave search nonexistent-pack-xyz` | Exits 0 with empty/no results (not an error) |

**Pass criteria:** Both search modes return results without errors.

---

## Flow 8: Cleanup

**Goal:** Restore machine to pre-test state.

| Step | Command | Expected |
|------|---------|----------|
| 8.1 | `weave use default` | Exits 0 |
| 8.2 | `weave profile delete e2e-validation` | Exits 0 |
| 8.3 | `weave remove e2e-local-test 2>/dev/null \|\| true` | Pack removed if present |
| 8.4 | `rm -rf /tmp/weave-e2e-local` | Directory removed |
| 8.5 | `weave list` | Clean — no e2e test packs |
| 8.6 | `weave diagnose` | No errors |

**Pass criteria:** Machine state matches pre-test baseline.

---

## Summary table template

| Flow | Status | Notes |
|------|--------|-------|
| 1 — Environment | ✓ / ✗ | |
| 2 — Registry install | ✓ / ✗ | |
| 3 — Diagnose | ✓ / ✗ | |
| 4 — Local install | ✓ / ✗ | |
| 5 — Profiles | ✓ / ✗ | |
| 6 — Non-destructive remove | ✓ / ✗ | |
| 7 — Search | ✓ / ✗ | |
| 8 — Cleanup | ✓ / ✗ | |
