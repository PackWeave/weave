//! Core install orchestration — registry + store + adapter apply flow.
//!
//! All business logic lives here; the CLI handler is a thin wrapper that
//! parses arguments, calls these functions, and formats output.

use std::collections::HashMap;
use std::path::Path;

use crate::adapters::{ApplyOptions, CliAdapter};
use crate::core::config::Config;
use crate::core::conflict;
use crate::core::lockfile::LockFile;
use crate::core::pack::{Pack, PackSource, ResolvedPack};
use crate::core::profile::{InstalledPack, Profile};
use crate::core::registry::{PackRelease, Registry};
use crate::core::resolver::Resolver;
use crate::core::store::Store;
use crate::error::{Result, WeaveError};

/// Returns the names of adapters that a pack would target, based on the pack's
/// `targets` flags and which adapters are installed.
pub fn target_adapters(pack: &Pack, adapters: &[Box<dyn CliAdapter>]) -> Vec<String> {
    use crate::adapters::AdapterId;
    adapters
        .iter()
        .filter(|a| match a.id() {
            AdapterId::ClaudeCode => pack.targets.claude_code,
            AdapterId::GeminiCli => pack.targets.gemini_cli,
            AdapterId::CodexCli => pack.targets.codex_cli,
        })
        .map(|a| a.name().to_string())
        .collect()
}

/// Result of installing a single pack — used for per-pack reporting.
#[derive(Debug)]
pub struct PackInstallResult {
    pub name: String,
    pub version: semver::Version,
    /// Adapters that the pack was successfully applied to.
    pub applied_adapters: Vec<String>,
    /// Adapter errors (non-fatal warnings).
    pub adapter_errors: Vec<String>,
    /// Tool conflicts detected (informational).
    pub tool_conflicts: Vec<String>,
    /// Required env vars that are not set.
    pub missing_env_vars: Vec<MissingEnvVar>,
    /// Whether the pack declares hooks.
    pub has_hooks: bool,
}

/// A required env var that is not set.
#[derive(Debug)]
pub struct MissingEnvVar {
    pub pack_name: String,
    pub key: String,
    pub description: Option<String>,
}

/// Overall result of a registry install operation.
#[derive(Debug)]
pub struct InstallResult {
    /// Packs that were already installed and up to date.
    pub already_satisfied: Vec<String>,
    /// Per-pack results for packs that were installed.
    pub installed: Vec<PackInstallResult>,
}

/// Shared context for install operations — groups the mutable state and
/// dependencies that would otherwise require too many function arguments.
pub struct InstallContext<'a> {
    pub config: &'a Config,
    pub registry: &'a dyn Registry,
    pub profile: &'a mut Profile,
    pub lockfile: &'a mut LockFile,
    pub adapters: &'a [Box<dyn CliAdapter>],
}

/// Install a pack from the registry (not local), applying it to the given adapters.
///
/// Handles dependency resolution, fetching from registry, storing locally,
/// conflict checking, adapter apply, and profile/lockfile recording.
pub fn install_from_registry(
    pack_name: &str,
    version_req: Option<&semver::VersionReq>,
    force: bool,
    options: &ApplyOptions,
    ctx: &mut InstallContext<'_>,
    dry_run: bool,
) -> Result<InstallResult> {
    let resolver = Resolver::new(ctx.registry);
    let plan = resolver.plan_install(pack_name, version_req, ctx.profile)?;

    if plan.to_install.is_empty() {
        return Ok(InstallResult {
            already_satisfied: plan.already_satisfied,
            installed: vec![],
        });
    }

    // Load installed pack manifests once before the loop rather than on each
    // iteration — avoids redundant I/O when installing multiple packs.
    let installed_packs = if !force {
        load_installed_packs(ctx.profile)
    } else {
        vec![]
    };

    let mut results = Vec::new();

    for (name, version) in &plan.to_install {
        // Fetch pack metadata from registry.
        let release = ctx.registry.fetch_version(name, version)?;

        if dry_run {
            // In dry-run mode, parse pack.toml directly from registry data
            // without writing to the local store.
            let pack_toml =
                release
                    .files
                    .get("pack.toml")
                    .ok_or_else(|| WeaveError::InvalidManifest {
                        path: std::path::PathBuf::from(format!("{name}/pack.toml")),
                        reason: "registry release missing pack.toml".to_string(),
                    })?;
            let pack = Pack::from_toml(pack_toml, &std::path::PathBuf::from("pack.toml"))?;

            // Validate manifest matches what was resolved, same as the normal path.
            if pack.name != *name {
                return Err(WeaveError::ManifestMismatch {
                    field: "name",
                    expected: name.clone(),
                    actual: pack.name,
                });
            }
            if pack.version != *version {
                return Err(WeaveError::ManifestMismatch {
                    field: "version",
                    expected: version.to_string(),
                    actual: pack.version.to_string(),
                });
            }

            let tool_conflicts = if !force {
                conflict::check_tool_conflicts(&pack, &installed_packs)
                    .iter()
                    .map(|c| {
                        format!(
                            "tool conflict: '{}' is exported by both {}/{} and {}/{}",
                            c.tool_name,
                            c.installed_pack,
                            c.installed_server,
                            c.incoming_pack,
                            c.incoming_server,
                        )
                    })
                    .collect()
            } else {
                vec![]
            };

            let applied_adapters = target_adapters(&pack, ctx.adapters);
            results.push(PackInstallResult {
                name: name.clone(),
                version: version.clone(),
                applied_adapters,
                adapter_errors: vec![],
                tool_conflicts,
                missing_env_vars: check_missing_env_vars(&pack),
                has_hooks: pack.has_hooks(),
            });
            continue;
        }

        // Normal path: fetch to local store and apply.
        let pack_dir = Store::fetch(name, &release, None)?;

        // Load the pack manifest
        let pack = Pack::load(&pack_dir)?;

        // Validate that the manifest matches what was resolved.
        if pack.name != *name {
            return Err(WeaveError::ManifestMismatch {
                field: "name",
                expected: name.clone(),
                actual: pack.name,
            });
        }
        if pack.version != *version {
            return Err(WeaveError::ManifestMismatch {
                field: "version",
                expected: version.to_string(),
                actual: pack.version.to_string(),
            });
        }

        // Check for tool-name conflicts with already-installed packs.
        let tool_conflicts = if !force {
            conflict::check_tool_conflicts(&pack, &installed_packs)
                .iter()
                .map(|c| {
                    format!(
                        "tool conflict: '{}' is exported by both {}/{} and {}/{}",
                        c.tool_name,
                        c.installed_pack,
                        c.installed_server,
                        c.incoming_pack,
                        c.incoming_server,
                    )
                })
                .collect()
        } else {
            vec![]
        };

        let resolved = ResolvedPack {
            pack: pack.clone(),
            source: PackSource::Registry {
                registry_url: ctx.config.registry_url.clone(),
            },
        };

        // Check for missing required env vars.
        let has_hooks = pack.has_hooks();
        let missing_env_vars = check_missing_env_vars(&pack);

        {
            // Apply to each installed adapter.
            let (applied_adapters, adapter_errors) =
                apply_to_adapters(&resolved, ctx.adapters, options);

            // Only record in profile/lockfile if at least one adapter succeeded
            // (empty applied_adapters + errors means rollback occurred).
            let rollback_occurred = applied_adapters.is_empty() && !adapter_errors.is_empty();
            if !rollback_occurred {
                // Record in profile
                ctx.profile.add_pack(InstalledPack {
                    name: name.clone(),
                    version: version.clone(),
                    source: PackSource::Registry {
                        registry_url: ctx.config.registry_url.clone(),
                    },
                });

                // Record in lock file
                ctx.lockfile.lock_pack(
                    name,
                    version.clone(),
                    PackSource::Registry {
                        registry_url: ctx.config.registry_url.clone(),
                    },
                );
            }

            results.push(PackInstallResult {
                name: name.clone(),
                version: version.clone(),
                applied_adapters,
                adapter_errors,
                tool_conflicts,
                missing_env_vars,
                has_hooks,
            });
        }
    }

    // Save state (skip in dry-run mode)
    if !dry_run {
        ctx.profile.save()?;
        ctx.lockfile.save(&ctx.config.active_profile)?;
    }

    Ok(InstallResult {
        already_satisfied: plan.already_satisfied,
        installed: results,
    })
}

/// Result of a local install operation.
#[derive(Debug)]
pub struct LocalInstallResult {
    pub name: String,
    pub version: semver::Version,
    pub applied_adapters: Vec<String>,
    pub adapter_errors: Vec<String>,
    pub tool_conflicts: Vec<String>,
    pub missing_env_vars: Vec<MissingEnvVar>,
    /// Dependency names declared but not auto-resolved.
    pub unresolved_dependencies: Vec<String>,
    /// Whether the pack declares hooks.
    pub has_hooks: bool,
}

/// Install a pack from a local directory path (bypasses the registry).
///
/// Reads `pack.toml` and all files from the directory, writes them to the
/// store, and applies the pack to all installed CLI adapters.
pub fn install_local(
    path: &Path,
    force: bool,
    options: &ApplyOptions,
    ctx: &mut InstallContext<'_>,
    dry_run: bool,
) -> Result<LocalInstallResult> {
    let pack = Pack::load(path)?;

    let name = &pack.name;
    let version = &pack.version;

    let unresolved_dependencies: Vec<String> = pack.dependencies.keys().cloned().collect();

    let local_source = PackSource::Local {
        path: path.to_string_lossy().to_string(),
    };

    if dry_run {
        // In dry-run mode, use the pack already loaded from disk — skip
        // store eviction and fetch to avoid any filesystem mutations.
        let tool_conflicts = if !force {
            let installed_packs: Vec<Pack> = load_installed_packs(ctx.profile)
                .into_iter()
                .filter(|p| p.name != *name)
                .collect();
            conflict::check_tool_conflicts(&pack, &installed_packs)
                .iter()
                .map(|c| {
                    format!(
                        "tool conflict: '{}' is exported by both {}/{} and {}/{}",
                        c.tool_name,
                        c.installed_pack,
                        c.installed_server,
                        c.incoming_pack,
                        c.incoming_server,
                    )
                })
                .collect()
        } else {
            vec![]
        };

        return Ok(LocalInstallResult {
            name: name.clone(),
            version: version.clone(),
            applied_adapters: target_adapters(&pack, ctx.adapters),
            adapter_errors: vec![],
            tool_conflicts,
            missing_env_vars: check_missing_env_vars(&pack),
            unresolved_dependencies,
            has_hooks: pack.has_hooks(),
        });
    }

    // Local installs always re-install, even at the same version, so that
    // file changes made during pack development are picked up without
    // requiring a version bump.
    Store::evict(name, version, Some(&local_source))?;

    let files = files_from_dir(path)?;

    let release = PackRelease {
        version: version.clone(),
        files,
        dependencies: pack.dependencies.clone(),
    };

    let pack_dir = Store::fetch(name, &release, Some(&local_source))?;

    // Re-load from store to validate written files.
    let pack = Pack::load(&pack_dir)?;

    let resolved = ResolvedPack {
        pack: pack.clone(),
        source: local_source.clone(),
    };

    // Exclude the pack being refreshed from conflict detection — otherwise a
    // re-install of the same pack would always flag self-conflicts.
    let installed_packs = if !force {
        load_installed_packs(ctx.profile)
            .into_iter()
            .filter(|p| p.name != *name)
            .collect()
    } else {
        vec![]
    };

    let tool_conflicts = if !force {
        conflict::check_tool_conflicts(&pack, &installed_packs)
            .iter()
            .map(|c| {
                format!(
                    "tool conflict: '{}' is exported by both {}/{} and {}/{}",
                    c.tool_name,
                    c.installed_pack,
                    c.installed_server,
                    c.incoming_pack,
                    c.incoming_server,
                )
            })
            .collect()
    } else {
        vec![]
    };

    let has_hooks = pack.has_hooks();
    let missing_env_vars = check_missing_env_vars(&pack);

    let (applied_adapters, adapter_errors) = apply_to_adapters(&resolved, ctx.adapters, options);

    // Only record and save if adapters didn't all roll back.
    let rollback_occurred = applied_adapters.is_empty() && !adapter_errors.is_empty();
    if !rollback_occurred {
        // Remove old version from profile if upgrading.
        ctx.profile.remove_pack(name);
        ctx.profile.add_pack(InstalledPack {
            name: name.clone(),
            version: version.clone(),
            source: local_source.clone(),
        });
        ctx.lockfile.lock_pack(name, version.clone(), local_source);

        ctx.profile.save()?;
        ctx.lockfile.save(&ctx.config.active_profile)?;
    }

    Ok(LocalInstallResult {
        name: name.clone(),
        version: version.clone(),
        applied_adapters,
        adapter_errors,
        tool_conflicts,
        missing_env_vars,
        unresolved_dependencies,
        has_hooks,
    })
}

/// Apply a resolved pack to all given adapters. Returns (successes, errors).
///
/// If any adapter fails, all previously successful adapters are rolled back
/// by calling `remove()`. In that case, `applied` is returned empty so that
/// callers know not to record the pack as installed.
pub fn apply_to_adapters(
    resolved: &ResolvedPack,
    adapters: &[Box<dyn CliAdapter>],
    options: &ApplyOptions,
) -> (Vec<String>, Vec<String>) {
    let mut applied: Vec<(String, &Box<dyn CliAdapter>)> = Vec::new();
    let mut errors = Vec::new();
    for adapter in adapters {
        match adapter.apply(resolved, options) {
            Ok(()) => applied.push((adapter.name().to_string(), adapter)),
            Err(e) => {
                let failed_name = adapter.name().to_string();
                errors.push(format!("{failed_name}: {e}"));

                // Roll back all previously successful adapters.
                for (name, successful_adapter) in &applied {
                    match successful_adapter.remove(&resolved.pack.name) {
                        Ok(_warnings) => {
                            errors.push(format!(
                                "{name}: applied then rolled back due to {failed_name} failure"
                            ));
                        }
                        Err(rollback_err) => {
                            log::warn!(
                                "rollback of {} from {name} failed: {rollback_err}",
                                resolved.pack.name
                            );
                            errors.push(format!(
                                "{name}: applied then rollback failed ({rollback_err}) — run `weave sync` to repair"
                            ));
                        }
                    }
                }

                // Return empty applied list — nothing should be recorded.
                return (vec![], errors);
            }
        }
    }
    let applied_names = applied.into_iter().map(|(name, _)| name).collect();
    (applied_names, errors)
}

/// Check for required env vars that are not set in the current environment.
pub fn check_missing_env_vars(pack: &Pack) -> Vec<MissingEnvVar> {
    let mut missing = Vec::new();
    for server in &pack.servers {
        for (key, env_var) in &server.env {
            if env_var.required && std::env::var_os(key).is_none() {
                missing.push(MissingEnvVar {
                    pack_name: pack.name.clone(),
                    key: key.clone(),
                    description: env_var.description.clone(),
                });
            }
        }
    }
    missing
}

/// Load pack manifests for all currently-installed packs from the local store.
/// Packs that cannot be loaded are skipped with a warning.
pub fn load_installed_packs(profile: &Profile) -> Vec<Pack> {
    let mut packs = Vec::new();
    for installed in &profile.packs {
        match Store::load_pack(&installed.name, &installed.version, Some(&installed.source)) {
            Ok(pack) => packs.push(pack),
            Err(e) => {
                log::warn!(
                    "could not load manifest for {} v{}: {e}",
                    installed.name,
                    installed.version
                );
            }
        }
    }
    packs
}

/// Return true if `s` looks like a filesystem path rather than a pack name.
pub fn is_local_path(s: &str) -> bool {
    s.starts_with('.') || s.starts_with('/') || s.starts_with('~') || Path::new(s).is_absolute()
}

/// Expand a leading `~` to the user's home directory.
pub fn expand_home(s: &str) -> std::path::PathBuf {
    if let Some(rest) = s.strip_prefix("~/")
        && let Some(home) = dirs::home_dir()
    {
        return home.join(rest);
    } else if s == "~"
        && let Some(home) = dirs::home_dir()
    {
        return home;
    }
    std::path::PathBuf::from(s)
}

/// Top-level directories that may contain pack content.
/// Files outside these paths (e.g. `target/`, `node_modules/`) are ignored.
const PACK_CONTENT_DIRS: &[&str] = &["prompts", "commands", "skills", "settings"];

/// Walk `dir` and return a flat map of `relative-path -> file content`.
///
/// Only includes `pack.toml` at the root and files under the known pack
/// content directories (`prompts/`, `commands/`, `skills/`, `settings/`).
/// Hidden entries and symlinks are skipped.
pub fn files_from_dir(dir: &Path) -> Result<HashMap<String, String>> {
    let mut files = HashMap::new();
    visit_dir(dir, dir, &mut files)?;
    Ok(files)
}

fn visit_dir(root: &Path, current: &Path, files: &mut HashMap<String, String>) -> Result<()> {
    let entries = std::fs::read_dir(current)
        .map_err(|e| WeaveError::io(format!("reading directory {}", current.display()), e))?;

    for entry in entries {
        let entry = entry
            .map_err(|e| WeaveError::io(format!("reading entry in {}", current.display()), e))?;
        let path = entry.path();
        let name = entry.file_name();
        let name_str = name.to_string_lossy();

        // Skip hidden entries (e.g. .git, .DS_Store).
        if name_str.starts_with('.') {
            continue;
        }

        // DirEntry::file_type() does not follow symlinks on any platform, so
        // is_symlink() correctly identifies symlinks and we skip them.
        let file_type = entry
            .file_type()
            .map_err(|e| WeaveError::io(format!("reading file type for {}", path.display()), e))?;
        if file_type.is_symlink() {
            continue;
        }

        if file_type.is_dir() {
            // At the root level, only recurse into known pack content directories.
            if current == root && !PACK_CONTENT_DIRS.contains(&name_str.as_ref()) {
                continue;
            }
            visit_dir(root, &path, files)?;
        } else {
            let rel = path
                .strip_prefix(root)
                .expect("path is always under root")
                .to_string_lossy()
                .replace('\\', "/");

            // At the root level, only include pack.toml.
            if current == root && rel != "pack.toml" {
                continue;
            }

            let content = std::fs::read_to_string(&path)
                .map_err(|e| WeaveError::io(format!("reading {}", path.display()), e))?;
            files.insert(rel, content);
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::adapters::{AdapterId, ApplyOptions, CliAdapter, DiagnosticIssue};
    use crate::core::pack::{Pack, PackSource, PackTargets, ResolvedPack};
    use std::collections::HashSet;
    use std::path::PathBuf;

    /// A mock adapter that succeeds on apply.
    struct SucceedAdapter {
        adapter_name: &'static str,
    }

    impl SucceedAdapter {
        fn new(name: &'static str) -> Self {
            Self { adapter_name: name }
        }
    }

    impl CliAdapter for SucceedAdapter {
        fn id(&self) -> AdapterId {
            AdapterId::ClaudeCode
        }
        fn name(&self) -> &str {
            self.adapter_name
        }
        fn is_installed(&self) -> bool {
            true
        }
        fn config_dir(&self) -> PathBuf {
            PathBuf::from("/tmp/mock")
        }
        fn apply(&self, _pack: &ResolvedPack, _options: &ApplyOptions) -> crate::error::Result<()> {
            Ok(())
        }
        fn remove(&self, _pack_name: &str) -> crate::error::Result<Vec<String>> {
            Ok(vec![])
        }
        fn diagnose(&self) -> crate::error::Result<Vec<DiagnosticIssue>> {
            Ok(vec![])
        }
        fn tracked_packs(&self) -> crate::error::Result<HashSet<String>> {
            Ok(HashSet::new())
        }
    }

    /// A mock adapter that always fails on apply.
    struct FailAdapter {
        adapter_name: &'static str,
    }

    impl CliAdapter for FailAdapter {
        fn id(&self) -> AdapterId {
            AdapterId::GeminiCli
        }
        fn name(&self) -> &str {
            self.adapter_name
        }
        fn is_installed(&self) -> bool {
            true
        }
        fn config_dir(&self) -> PathBuf {
            PathBuf::from("/tmp/mock")
        }
        fn apply(&self, _pack: &ResolvedPack, _options: &ApplyOptions) -> crate::error::Result<()> {
            Err(crate::error::WeaveError::ApplyFailed {
                pack: "test-pack".to_string(),
                cli: self.adapter_name.to_string(),
                reason: "simulated failure".to_string(),
            })
        }
        fn remove(&self, _pack_name: &str) -> crate::error::Result<Vec<String>> {
            Ok(vec![])
        }
        fn diagnose(&self) -> crate::error::Result<Vec<DiagnosticIssue>> {
            Ok(vec![])
        }
        fn tracked_packs(&self) -> crate::error::Result<HashSet<String>> {
            Ok(HashSet::new())
        }
    }

    fn make_resolved_pack() -> ResolvedPack {
        ResolvedPack {
            pack: Pack {
                name: "test-pack".to_string(),
                version: semver::Version::new(1, 0, 0),
                description: "A test pack".to_string(),
                authors: vec![],
                license: None,
                repository: None,
                keywords: vec![],
                min_tool_version: None,
                servers: vec![],
                dependencies: Default::default(),
                extensions: Default::default(),
                targets: PackTargets::default(),
            },
            source: PackSource::Local {
                path: "/tmp/test".to_string(),
            },
        }
    }

    #[test]
    fn apply_to_adapters_all_succeed() {
        let adapters: Vec<Box<dyn CliAdapter>> = vec![
            Box::new(SucceedAdapter::new("Adapter A")),
            Box::new(SucceedAdapter::new("Adapter B")),
        ];
        let resolved = make_resolved_pack();
        let options = ApplyOptions::default();

        let (applied, errors) = apply_to_adapters(&resolved, &adapters, &options);

        assert_eq!(applied, vec!["Adapter A", "Adapter B"]);
        assert!(errors.is_empty());
    }

    #[test]
    fn apply_to_adapters_rolls_back_on_partial_failure() {
        let adapters: Vec<Box<dyn CliAdapter>> = vec![
            Box::new(SucceedAdapter::new("Claude Code")),
            Box::new(FailAdapter {
                adapter_name: "Gemini CLI",
            }),
        ];
        let resolved = make_resolved_pack();
        let options = ApplyOptions::default();

        let (applied, errors) = apply_to_adapters(&resolved, &adapters, &options);

        // applied should be empty because rollback occurred
        assert!(
            applied.is_empty(),
            "expected empty applied list after rollback, got: {applied:?}"
        );

        // Should have errors: one for the failure, one for the rollback
        assert!(
            errors.len() >= 2,
            "expected at least 2 errors (failure + rollback), got: {errors:?}"
        );

        // The first error should be from the failing adapter
        assert!(
            errors[0].contains("Gemini CLI"),
            "first error should mention failing adapter: {errors:?}"
        );

        // The second error should mention the rollback
        assert!(
            errors[1].contains("rolled back"),
            "second error should mention rollback: {errors:?}"
        );
    }

    #[test]
    fn apply_to_adapters_returns_empty_on_first_adapter_failure() {
        let adapters: Vec<Box<dyn CliAdapter>> = vec![
            Box::new(FailAdapter {
                adapter_name: "Gemini CLI",
            }),
            Box::new(SucceedAdapter::new("Claude Code")),
        ];
        let resolved = make_resolved_pack();
        let options = ApplyOptions::default();

        let (applied, errors) = apply_to_adapters(&resolved, &adapters, &options);

        // applied should be empty — the first adapter failed before any succeeded
        assert!(applied.is_empty());
        // Only one error — the failure itself, no rollbacks needed
        assert_eq!(errors.len(), 1);
        assert!(errors[0].contains("Gemini CLI"));
    }
}
