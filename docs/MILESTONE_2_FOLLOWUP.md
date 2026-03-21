# Milestone 2 Follow-up

This document captures the state of Milestone 2 at the point it was merged into `main`. It records what was fully implemented, what deviated from the original spec and why, and what was not yet completed. It is intended to inform the start of Milestone 3 and to surface any work that should be resolved before that milestone begins.

---

## Merge-time state of Milestone 2

Milestone 2 is **functionally complete** — the full install/list/remove flow is implemented end-to-end, both adapters are production-quality, and CI passes on Ubuntu and macOS. Three items from the roadmap checklist are not yet present in this repository and block a public release.

| # | Roadmap item | Status |
|---|--------------|--------|
| 1 | `cargo init` with correct crate name and metadata | ✅ Done |
| 2 | Core commands: `install`, `list`, `remove` | ✅ Done |
| 3 | Pack manifest parsing + validation (TOML) | ✅ Done |
| 4 | Local store: extract and cache packs | ✅ Done |
| 5 | Lock file for pinned versions | ✅ Done |
| 6 | GitHub-backed registry index (read-only) | ✅ Done |
| 7 | Seed registry with 10–15 starter packs | ❌ Missing |
| 8 | Claude Code adapter (servers, prompts, commands, settings) | ✅ Done |
| 9 | Gemini CLI adapter (servers, prompts, settings) | ✅ Done |
| 10 | One-line install script | ❌ Missing |
| 11 | Homebrew formula | ❌ Missing |
| 12 | CI: build + clippy + fmt check on push | ✅ Done |

---

## Deviations from the architecture spec

### 1. Transactional install and remove — pragmatic shortcut

**What was specified:** ARCHITECTURE.md implies atomic operations — the store, profile, and lock file should be consistent at all times. If adapter application fails, the system should not be left in a partially-applied state.

**What was implemented:** Both `cli/install.rs` and `cli/remove.rs` use a collect-and-continue strategy. If one adapter fails, earlier adapters may have already written config changes, but the pack is still recorded in the profile and lock file. Errors are surfaced as warnings rather than returned as failures.

**Classification:** Pragmatic shortcut.

**Rationale:** Failing the entire install when one of N adapters encounters a bad config file is worse UX than recording the intent and warning. The profile/lock file always reflects what was attempted, which is required for subsequent `remove` to be able to clean up.

**Risk if left unaddressed:**
- A pack can appear "installed" in the profile even if one adapter did not fully apply it. `weave doctor` (Milestone 4) is the planned mitigation.
- If an adapter fails mid-apply (e.g. after writing servers but before writing settings), the adapter's own state may be partially applied with no record in the sidecar manifest of what was written. This can leave orphaned entries in CLI config files.

**Recommendation:** Before Milestone 3 begins, audit whether adapter `apply()` is itself atomic within a single adapter (all-or-nothing within one CLI). Cross-adapter rollback can remain deferred.

---

### 2. Recursive dependency resolution — intentional deferral

**What was specified:** `Resolver` produces a flat `InstallPlan` including transitive dependencies.

**What was implemented:** Dependencies declared in `pack.toml` are parsed and stored in `Pack.dependencies`, but `plan_install()` does not resolve them transitively. A comment at `resolver.rs:116` explicitly documents this.

**Classification:** Intentional deferral.

**Risk if left unaddressed:**
- If a published pack declares `[dependencies]`, installing it will silently skip those dependencies. The pack may fail at runtime inside the AI CLI if it expects another pack's servers or prompts to be present.
- No user-facing warning is emitted when a pack with dependencies is installed.

**Recommendation:** Before publishing any pack to the registry that uses `[dependencies]`, either implement recursive resolution or emit a clear warning at install time that transitive dependencies are not auto-resolved in v0.1.

---

### 3. Project-scope applied only if directory exists at install time — intentional

**What was implemented:** Both adapters check `has_project_scope()` before writing project-scoped config. `has_project_scope()` returns true only if `.claude/` (or `.gemini/`) already exists in the current working directory at the moment `weave install` is run.

**Classification:** Intentional — consistent with the architecture principle that profiles are explicit and there is no implicit drift.

**Risk if left unaddressed:**
- A user who runs `weave install` at the project root before Claude Code has created `.claude/` will get user-scope config only. The pack will not be applied to the project scope even after `.claude/` is later created.
- Re-running `weave install <pack>` (or a future `weave sync`) will then pick it up. The behaviour is correct but not obvious.

**Recommendation:** Document this in the README and in the output of `weave install` when project-scope is skipped. No code change needed before Milestone 3.

---

### 4. `weave search` implemented ahead of schedule — minor scope creep

**What was implemented:** `src/cli/search.rs` and the `Search` subcommand in `main.rs` provide a basic `weave search <query>` command that queries the weave pack registry.

**What was specified:** `weave search` is listed under Milestone 3 ("against the official MCP Registry").

**Classification:** Scope creep — but benign. The current implementation searches the weave registry only (not the upstream MCP Registry), which is narrower than the Milestone 3 spec.

**Risk if left unaddressed:**
- None in isolation. The Milestone 3 `weave search` will need to either extend this implementation or replace it. If the interface is different (e.g. Milestone 3 adds `--registry` flags or returns richer MCP metadata), a breaking change to the CLI surface is possible.

**Recommendation:** Keep as-is. When Milestone 3 begins, revisit the search interface design before publishing the CLI to ensure the Milestone 3 scope can be added without a breaking change.

---

## Missing items and their impact

### 1. Seeded registry — blocks all end-to-end functionality

**What is needed:** A `PackWeave/registry` GitHub repository with an `index.json` in the format consumed by `GitHubRegistry::load_index()`. The index must reference at least 10–15 starter packs as GitHub Releases assets with correct SHA256 checksums.

**Impact if absent:** Every call to `weave install`, `weave list` (when querying the registry), and `weave search` will fail with a network error or a 404 from `https://raw.githubusercontent.com/PackWeave/registry/main/index.json`. The tool is not usable by any end user until this exists.

**Impact on Milestone 3:** `weave update` and `weave search` (Milestone 3) both depend on the registry being populated. Milestone 3 work cannot be validated without it.

**Classification:** Blocking for any public release.

---

### 2. One-line install script — blocks adoption

**What is needed:** An `install.sh` (or equivalent) hosted at a stable URL that downloads the correct pre-built binary for the user's platform and architecture, verifies its checksum, and places it in `$PATH`.

**Impact if absent:** Users cannot install weave without building from source. This is acceptable for early contributors but prevents any broader testing or adoption.

**Classification:** Required for public release; not a technical blocker for Milestone 3 development.

---

### 3. Homebrew formula — blocks macOS adoption

**What is needed:** A Homebrew tap or formula (`Formula/weave.rb`) pointing at a tagged GitHub release tarball.

**Impact if absent:** macOS users have no `brew install` path. Combined with the absence of an install script, there is no smooth install path on any platform.

**Classification:** Required for public release; not a technical blocker for Milestone 3 development.

---

## Recommendations

### Must resolve before Milestone 3 begins

1. **Create the `PackWeave/registry` repo and seed it.** Without a real registry, no integration testing of the install/remove flow is possible, and Milestone 3 features (`weave update`, `weave search` against MCP Registry) cannot be validated against a real data set. This is the single highest-priority outstanding item.

2. **Audit intra-adapter atomicity.** Verify that if an adapter's `apply()` fails partway through (e.g. after writing servers but before writing settings), the sidecar manifest accurately reflects what was written so that a subsequent `remove()` can clean up correctly. If it does not, add a guard — either write to the manifest atomically with each operation, or collect all mutations and write them in a single final step.

3. **Emit a warning when dependencies are skipped.** Before any pack with a `[dependencies]` block reaches the registry, ensure `plan_install()` warns the user that transitive dependencies are not auto-resolved. This prevents silent misconfiguration.

### Can be deferred alongside Milestone 3 delivery

4. **Install script and Homebrew formula.** These are delivery mechanics, not functional blockers. They should be ready before the first public announcement, but Milestone 3 work does not depend on them.

5. **`weave search` interface review.** Revisit the CLI surface design at the start of Milestone 3 when the MCP Registry integration spec is written. No code change needed now.

6. **Cross-adapter rollback on install/remove failure.** The current collect-and-continue strategy is acceptable for v0.1. Full transactional rollback is complex and should be scoped as a Milestone 3 or 4 item, aligned with `weave doctor` and `weave sync`.

7. **Document project-scope detection behaviour.** A note in the README explaining that project-scope config is only applied if the `.claude/` or `.gemini/` directory already exists at install time. No code change.

---

*Document generated at Milestone 2 merge. Update when items are resolved.*
