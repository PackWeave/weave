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
| 2.2 | `weave install filesystem` | Exits 0, prints "Installing filesystem" and "Applied to Claude Code" |
| 2.3 | `cat ~/.claude.json \| jq '.mcpServers \| keys'` | Contains filesystem server key(s) |
| 2.4 | `weave list` | Shows `filesystem` with scope `user` |
| 2.5 | `weave install filesystem` | Exits 0, prints "already installed and up to date" (idempotent) |
| 2.6 | (If Gemini installed) `cat ~/.gemini/settings.json \| jq '.mcpServers \| keys'` | Contains filesystem |
| 2.7 | (If Codex installed) `cat ~/.codex/config.toml` | Contains filesystem entry |

**Pass criteria:** All installed CLIs show filesystem in their config. Repeat install is idempotent.

---

## Flow 3: Diagnose and drift recovery

**Goal:** Verify `weave diagnose` accurately reflects installed state, detects drift, and `weave sync` repairs it.

| Step | Command | Expected |
|------|---------|----------|
| 3.1 | `weave diagnose` | Exits 0; filesystem shows `ok` for installed CLIs, `skipped` for uninstalled |
| 3.2 | `weave diagnose --json` | Valid JSON; `"name": "filesystem"` present with `"status": "ok"` |
| 3.3 | `weave diagnose --json \| jq '.packs[0].adapters'` | Each adapter has `status` field; non-installed show `skipped` |
| 3.4 | Manually remove filesystem server from `~/.claude.json` mcpServers | Drift simulated |
| 3.5 | `weave diagnose` | Reports `drifted` for Claude Code with helpful message |
| 3.6 | `weave diagnose --json \| jq '.issue_count'` | Returns `1` (or more) |
| 3.7 | `weave sync` | Exits 0; prints "filesystem@... -> Claude Code" |
| 3.8 | `weave diagnose` | Back to `ok`, "No issues found" |

**Pass criteria:** Diagnose catches drift; sync repairs it; diagnose confirms recovery.

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

**Goal:** Verify profile create, add, use, switch, and isolation.

| Step | Command | Expected |
|------|---------|----------|
| 5.1 | `weave profile create e2e-validation` | Exits 0, "Created profile" |
| 5.2 | `weave profile list` | Shows `e2e-validation` in list |
| 5.3 | `weave profile create e2e-validation` | Exits 1, "already exists" |
| 5.4 | `weave use e2e-validation` | Exits 0; removes packs from prior profile, prints switch confirmation |
| 5.5 | `weave use` | Prints `e2e-validation` (active profile) |
| 5.6 | `weave list` | Empty (new profile has no packs) |
| 5.7 | `weave install filesystem` | Exits 0, applied to adapters |
| 5.8 | `weave profile add github --profile e2e-validation` | Exits 0, prints "Applied to Claude Code" (active profile auto-applies) |
| 5.9 | `cat ~/.claude.json \| jq '.mcpServers \| keys'` | Contains both filesystem and github server(s) |
| 5.10 | `weave list` | Shows both filesystem and github |
| 5.11 | `weave sync` | Exits 0, reapplies all packs, no errors |
| 5.12 | `weave use <original-profile>` | Switch back; removes e2e-validation packs from adapters |
| 5.13 | `cat ~/.claude.json \| jq '.mcpServers \| keys'` | filesystem and github absent (unless original profile had them) |

**Pass criteria:** Profile switch applies/removes correctly; `profile add` to active profile auto-applies; sync is clean.

---

## Flow 6: Non-destructive remove

**Goal:** Verify `weave remove` leaves user config intact.

| Step | Command | Expected |
|------|---------|----------|
| 6.1 | Note filesystem keys before removal: `weave list` | Baseline |
| 6.2 | `weave remove filesystem` | Exits 0 |
| 6.3 | `cat ~/.claude.json \| jq '.mcpServers \| keys'` | filesystem keys absent |
| 6.4 | `weave list` | filesystem no longer listed |
| 6.5 | Check other mcpServers keys unchanged | No regressions in user config |
| 6.6 | `weave remove filesystem` | Exits 1, "not installed" error |

**Pass criteria:** filesystem removed cleanly; no other config sections modified; removing again errors.

---

## Flow 7: Search

**Goal:** Verify registry and MCP search work.

| Step | Command | Expected |
|------|---------|----------|
| 7.1 | `weave search filesystem` | Returns results from weave registry |
| 7.2 | `weave search --mcp filesystem` | Contacts MCP Registry; returns results |
| 7.3 | `weave search nonexistent-pack-xyz` | Exits 0 with "No packs found" (not an error) |

**Pass criteria:** Both search modes return results without errors.

---

## Flow 8: Project-scope install and remove

**Goal:** Verify `--project` flag writes/cleans `.mcp.json` correctly.

**Prerequisite:** Must be in a directory with `.claude/` (to trigger project-scope detection).

| Step | Command | Expected |
|------|---------|----------|
| 8.1 | `weave install --project filesystem` | Exits 0, applied to Claude Code |
| 8.2 | `cat .mcp.json` | Contains filesystem server in `mcpServers` |
| 8.3 | `weave list` | Shows filesystem with scope `user + project (...)` |
| 8.4 | `weave diagnose` | filesystem shows `ok` |
| 8.5 | `weave remove filesystem` | Exits 0 |
| 8.6 | `ls .mcp.json 2>/dev/null` | File should NOT exist (deleted when empty, not left as empty stub) |

**Pass criteria:** Project-scope install creates `.mcp.json`; remove deletes it when empty.

---

## Flow 9: Update

**Goal:** Verify `weave update` checks and applies updates.

| Step | Command | Expected |
|------|---------|----------|
| 9.1 | `weave install filesystem` | Exits 0 |
| 9.2 | `weave update filesystem` | Exits 0, prints "already up to date" or applies update |
| 9.3 | `weave update` | Exits 0, checks all installed packs |
| 9.4 | `weave list` | Pack versions match latest available |

**Pass criteria:** Update checks all installed packs without errors.

---

## Flow 11: Community taps

**Goal:** Verify `weave tap add/list/remove` commands work against real GitHub-hosted taps.

**Prerequisite:** `PackWeave/example-tap` GitHub repo must exist with a `tap-test` pack.

| Step | Command | Expected |
|------|---------|----------|
| 11.1 | `weave tap list` | "No community taps registered" or shows existing taps |
| 11.2 | `weave tap add PackWeave/example-tap` | Exits 0, prints "Tap 'PackWeave/example-tap' added" with URL |
| 11.3 | `weave tap list` | Shows `PackWeave/example-tap` with `https://raw.githubusercontent.com/PackWeave/example-tap/main` |
| 11.4 | `weave tap add PackWeave/example-tap` | Exits 1, "already registered" |
| 11.5 | `weave install tap-test` | Exits 0, pack resolves from the tap |
| 11.6 | `weave list` | Shows `tap-test` |
| 11.7 | `weave remove tap-test` | Exits 0, clean removal |
| 11.8 | `weave tap remove PackWeave/example-tap` | Exits 0, prints removal confirmation |
| 11.9 | `weave tap list` | "No community taps registered" |

**Pass criteria:** Tap lifecycle (add → install from tap → remove tap) works end-to-end.

---

## Flow 12: Hooks (`--allow-hooks`)

**Goal:** Verify hooks opt-in, application, badge display, and cleanup.

**Setup:** Create a local pack with hooks at `/tmp/weave-e2e-hooks/`

```toml
# /tmp/weave-e2e-hooks/pack.toml
[pack]
name = "e2e-hooks-test"
version = "0.1.0"
description = "E2E hooks test"
authors = ["e2e-tester"]

[extensions.claude_code.hooks]
PreToolUse = [{ matcher = "Bash", command = "echo e2e-hook-fired" }]
```

| Step | Command | Expected |
|------|---------|----------|
| 12.1 | Create pack dir + `pack.toml` as above | Files exist |
| 12.2 | `weave install /tmp/weave-e2e-hooks` | Exits 0, output notes the pack declares hooks and instructs to pass `--allow-hooks` to apply them |
| 12.3 | `weave list` | Shows `e2e-hooks-test` with `[hooks]` badge |
| 12.4 | `cat ~/.claude/settings.json 2>/dev/null \| jq '.hooks // empty'` | No hooks key (skipped without flag) |
| 12.5 | `weave remove e2e-hooks-test` | Clean removal |
| 12.6 | `weave install /tmp/weave-e2e-hooks --allow-hooks` | Exits 0, hooks applied |
| 12.7 | `cat ~/.claude/settings.json \| jq '.hooks'` | Contains `PreToolUse` array with matcher "Bash" and command |
| 12.8 | `weave remove e2e-hooks-test` | Exits 0 |
| 12.9 | `cat ~/.claude/settings.json \| jq '.hooks // "absent"'` | Hooks key removed or absent |

**Pass criteria:** Hooks are skipped without flag, applied with flag, cleaned up on remove.

---

## Flow 13: HTTP transport servers

**Goal:** Verify remote MCP servers with HTTP transport and headers are written correctly to CLI configs.

**Setup:** Create a local pack with an HTTP transport server at `/tmp/weave-e2e-http/`

```toml
# /tmp/weave-e2e-http/pack.toml
[pack]
name = "e2e-http-test"
version = "0.1.0"
description = "E2E HTTP transport test"
authors = ["e2e-tester"]

[[servers]]
name = "e2e-remote"
transport = "http"
url = "https://example.com/mcp/e2e-test"

[servers.headers]
Authorization = "Bearer ${E2E_API_KEY}"
X-Test = "static-value"
```

| Step | Command | Expected |
|------|---------|----------|
| 13.1 | Create pack dir + `pack.toml` as above | Files exist |
| 13.2 | `weave install /tmp/weave-e2e-http` | Exits 0, applied to adapters |
| 13.3 | `cat ~/.claude.json \| jq '.mcpServers["e2e-remote"]'` | Has `"type": "http"`, `"url": "https://example.com/mcp/e2e-test"`, no `"command"` key |
| 13.4 | `cat ~/.claude.json \| jq '.mcpServers["e2e-remote"].headers'` | Has `Authorization` with `${E2E_API_KEY}` and `X-Test` with `static-value` |
| 13.5 | (If Codex installed) `grep -A5 'e2e-remote' ~/.codex/config.toml` | Shows `url` and `[mcp_servers.e2e-remote.http_headers]` section |
| 13.6 | `weave remove e2e-http-test` | Exits 0 |
| 13.7 | `cat ~/.claude.json \| jq '.mcpServers["e2e-remote"] // "absent"'` | Server removed |

**Pass criteria:** HTTP servers appear in config with type/url/headers (not command/args); clean removal.

---

## Flow 14: Cleanup

**Goal:** Restore machine to pre-test state.

| Step | Command | Expected |
|------|---------|----------|
| 14.1 | `weave remove filesystem 2>/dev/null \|\| true` | Removed if present |
| 14.2 | `weave remove github 2>/dev/null \|\| true` | Removed if present |
| 14.3 | `weave remove e2e-local-test 2>/dev/null \|\| true` | Removed if present |
| 14.4 | `weave remove e2e-hooks-test 2>/dev/null \|\| true` | Removed if present |
| 14.5 | `weave remove e2e-http-test 2>/dev/null \|\| true` | Removed if present |
| 14.6 | `weave remove tap-test 2>/dev/null \|\| true` | Removed if present |
| 14.7 | `weave tap remove PackWeave/example-tap 2>/dev/null \|\| true` | Tap removed |
| 14.8 | `weave use default` | Switch to default profile |
| 14.9 | `weave profile delete e2e-validation 2>/dev/null \|\| true` | Profile removed |
| 14.10 | `rm -rf /tmp/weave-e2e-local /tmp/weave-e2e-hooks /tmp/weave-e2e-http` | Temp dirs removed |
| 14.11 | `rm -f .mcp.json` | Project-scope file removed if present |
| 14.12 | `weave list` | Clean — no e2e test packs |
| 14.13 | `weave diagnose` | No errors |

**Pass criteria:** Machine state matches pre-test baseline.

---

## Summary table template

| Flow | Status | Notes |
|------|--------|-------|
| 1 — Environment | ✓ / ✗ | |
| 2 — Registry install | ✓ / ✗ | |
| 3 — Diagnose + drift recovery | ✓ / ✗ | |
| 4 — Local install | ✓ / ✗ | |
| 5 — Profiles | ✓ / ✗ | |
| 6 — Non-destructive remove | ✓ / ✗ | |
| 7 — Search | ✓ / ✗ | |
| 8 — Project-scope | ✓ / ✗ | |
| 9 — Update | ✓ / ✗ | |
| 11 — Community taps | ✓ / ✗ | |
| 12 — Hooks | ✓ / ✗ | |
| 13 — HTTP transport | ✓ / ✗ | |
| 14 — Cleanup | ✓ / ✗ | |
