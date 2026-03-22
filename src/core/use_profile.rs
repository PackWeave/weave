//! Core profile-switch orchestration — diff + remove + apply + fetch flow.
//!
//! All business logic lives here; the CLI handler is a thin wrapper that
//! parses arguments, calls these functions, and formats output.

use crate::adapters::{ApplyOptions, CliAdapter};
use crate::core::config::Config;
use crate::core::pack::{Pack, PackSource, ResolvedPack};
use crate::core::profile::{InstalledPack, Profile};
use crate::core::registry::{GitHubRegistry, Registry};
use crate::core::store::Store;

/// A pack removal result for a single adapter.
#[derive(Debug)]
pub struct PackRemoveResult {
    pub pack_name: String,
    /// Adapters that had the pack successfully removed.
    pub removed_adapters: Vec<String>,
    /// Per-adapter warnings (non-fatal).
    pub adapter_warnings: Vec<String>,
    /// Per-adapter errors (non-fatal).
    pub adapter_errors: Vec<String>,
}

/// A pack apply result for a single pack during profile switch.
#[derive(Debug)]
pub struct PackApplyResult {
    pub name: String,
    pub version: semver::Version,
    /// Adapters that had the pack successfully applied.
    pub applied_adapters: Vec<String>,
    /// Per-adapter errors (non-fatal).
    pub adapter_errors: Vec<String>,
    /// Non-None if the pack could not be loaded at all.
    pub load_error: Option<String>,
}

/// Overall result of a profile switch operation.
#[derive(Debug)]
pub struct SwitchResult {
    /// Packs that were removed during the switch.
    pub removed: Vec<PackRemoveResult>,
    /// Packs that were applied during the switch.
    pub applied: Vec<PackApplyResult>,
}

/// Compute the diff between two profiles.
/// Returns (packs_to_remove, packs_to_add).
///
/// - to_remove: packs in `current` but not in `target`
/// - to_add: packs in `target` but not in `current`
///
/// Packs present in both profiles (even at different versions) are handled by
/// removing the old and adding the new, which is correct because a pack present
/// in both with the same version will appear in neither list.
pub fn compute_diff(current: &Profile, target: &Profile) -> (Vec<String>, Vec<InstalledPack>) {
    let mut to_remove = Vec::new();
    let mut to_add = Vec::new();

    // Find packs to remove: in current but not in target, or version differs
    for pack in &current.packs {
        match target.get_pack(&pack.name) {
            None => to_remove.push(pack.name.clone()),
            Some(target_pack) if target_pack.version != pack.version => {
                to_remove.push(pack.name.clone());
            }
            _ => {} // Same version in both — no action needed
        }
    }

    // Find packs to add: in target but not in current, or version differs
    for pack in &target.packs {
        match current.get_pack(&pack.name) {
            None => to_add.push(pack.clone()),
            Some(current_pack) if current_pack.version != pack.version => {
                to_add.push(pack.clone());
            }
            _ => {} // Same version in both — no action needed
        }
    }

    (to_remove, to_add)
}

/// Try to load a pack from the store; if missing, attempt to fetch it from the registry.
pub fn load_or_fetch_pack(
    name: &str,
    version: &semver::Version,
    source: &PackSource,
) -> std::result::Result<Pack, anyhow::Error> {
    use anyhow::{Context, bail};

    // Try loading from store first
    if let Ok(pack) = Store::load_pack(name, version, Some(source)) {
        return Ok(pack);
    }

    // Try fetching from registry
    let registry_url = match source {
        PackSource::Registry { registry_url } => registry_url,
        _ => bail!("pack {name}@{version} not in local store and source is not a registry"),
    };

    let registry = GitHubRegistry::new(registry_url);
    let release = registry
        .fetch_version(name, version)
        .map_err(anyhow::Error::from)
        .context("resolving pack from registry")?;
    Store::fetch(name, &release, Some(source))
        .map_err(anyhow::Error::from)
        .context("downloading pack")?;
    Store::load_pack(name, version, Some(source))
        .map_err(anyhow::Error::from)
        .context("loading fetched pack")
}

/// Execute a profile switch: remove old packs, apply new packs, update config.
///
/// The caller is responsible for:
/// - Verifying the target profile exists
/// - Checking that the active profile is not already the target
/// - Printing output and formatting messages
pub fn switch(
    target_name: &str,
    config: &mut Config,
    current_profile: &Profile,
    target_profile: &Profile,
    adapters: &[Box<dyn CliAdapter>],
    options: &ApplyOptions,
) -> std::result::Result<SwitchResult, anyhow::Error> {
    use anyhow::Context;

    let (to_remove, to_add) = compute_diff(current_profile, target_profile);

    // Pre-flight: verify all packs can be loaded (or fetched) before making
    // any changes. Without this, the remove loop could run and then the add
    // loop could fail partway through, leaving adapter configs in a broken
    // state that is neither the old profile nor the new one.
    for installed in &to_add {
        load_or_fetch_pack(&installed.name, &installed.version, &installed.source)
            .with_context(|| {
                format!(
                    "cannot switch: pack {}@{} is not available — resolve this before switching profiles",
                    installed.name, installed.version
                )
            })?;
    }

    let mut result = SwitchResult {
        removed: vec![],
        applied: vec![],
    };

    // Remove packs that are in current but not in target
    for pack_name in &to_remove {
        let mut remove_result = PackRemoveResult {
            pack_name: pack_name.clone(),
            removed_adapters: vec![],
            adapter_warnings: vec![],
            adapter_errors: vec![],
        };

        for adapter in adapters {
            match adapter.remove(pack_name) {
                Ok(warnings) => {
                    remove_result
                        .removed_adapters
                        .push(adapter.name().to_string());
                    for w in warnings {
                        remove_result
                            .adapter_warnings
                            .push(format!("{}: {w}", adapter.name()));
                    }
                }
                Err(e) => {
                    remove_result
                        .adapter_errors
                        .push(format!("{}: {e}", adapter.name()));
                }
            }
        }

        result.removed.push(remove_result);
    }

    // Apply packs that are in target but not in current
    for installed in &to_add {
        let mut apply_result = PackApplyResult {
            name: installed.name.clone(),
            version: installed.version.clone(),
            applied_adapters: vec![],
            adapter_errors: vec![],
            load_error: None,
        };

        let pack = match load_or_fetch_pack(&installed.name, &installed.version, &installed.source)
        {
            Ok(p) => p,
            Err(e) => {
                apply_result.load_error = Some(format!(
                    "could not load {}@{}: {e}",
                    installed.name, installed.version
                ));
                result.applied.push(apply_result);
                continue;
            }
        };

        let resolved = ResolvedPack {
            pack,
            source: installed.source.clone(),
        };

        for adapter in adapters {
            match adapter.apply(&resolved, options) {
                Ok(()) => {
                    apply_result
                        .applied_adapters
                        .push(adapter.name().to_string());
                }
                Err(e) => {
                    apply_result
                        .adapter_errors
                        .push(format!("{}: {e}", adapter.name()));
                }
            }
        }

        result.applied.push(apply_result);
    }

    // Update the active profile in config
    config.active_profile = target_name.to_string();
    config
        .save()
        .map_err(anyhow::Error::from)
        .context("saving config")?;

    Ok(result)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::pack::PackSource;
    use crate::core::profile::InstalledPack;

    fn test_pack(name: &str, version: &str) -> InstalledPack {
        InstalledPack {
            name: name.to_string(),
            version: semver::Version::parse(version).unwrap(),
            source: PackSource::Registry {
                registry_url: "https://example.com".into(),
            },
        }
    }

    fn make_profile(name: &str, packs: Vec<InstalledPack>) -> Profile {
        Profile {
            name: name.to_string(),
            packs,
        }
    }

    #[test]
    fn diff_identical_profiles_is_empty() {
        let a = make_profile("a", vec![test_pack("webdev", "1.0.0")]);
        let b = make_profile("b", vec![test_pack("webdev", "1.0.0")]);
        let (remove, add) = compute_diff(&a, &b);
        assert!(remove.is_empty());
        assert!(add.is_empty());
    }

    #[test]
    fn diff_empty_to_populated() {
        let current = make_profile("current", vec![]);
        let target = make_profile(
            "target",
            vec![test_pack("webdev", "1.0.0"), test_pack("db", "2.0.0")],
        );
        let (remove, add) = compute_diff(&current, &target);
        assert!(remove.is_empty());
        assert_eq!(add.len(), 2);
        assert!(add.iter().any(|p| p.name == "webdev"));
        assert!(add.iter().any(|p| p.name == "db"));
    }

    #[test]
    fn diff_populated_to_empty() {
        let current = make_profile(
            "current",
            vec![test_pack("webdev", "1.0.0"), test_pack("db", "2.0.0")],
        );
        let target = make_profile("target", vec![]);
        let (remove, add) = compute_diff(&current, &target);
        assert_eq!(remove.len(), 2);
        assert!(remove.contains(&"webdev".to_string()));
        assert!(remove.contains(&"db".to_string()));
        assert!(add.is_empty());
    }

    #[test]
    fn diff_version_change_produces_remove_and_add() {
        let current = make_profile("current", vec![test_pack("webdev", "1.0.0")]);
        let target = make_profile("target", vec![test_pack("webdev", "2.0.0")]);
        let (remove, add) = compute_diff(&current, &target);
        assert_eq!(remove, vec!["webdev"]);
        assert_eq!(add.len(), 1);
        assert_eq!(add[0].name, "webdev");
        assert_eq!(add[0].version, semver::Version::new(2, 0, 0));
    }

    #[test]
    fn diff_mixed_add_remove_and_keep() {
        let current = make_profile(
            "current",
            vec![test_pack("webdev", "1.0.0"), test_pack("old-pack", "1.0.0")],
        );
        let target = make_profile(
            "target",
            vec![
                test_pack("webdev", "1.0.0"), // kept
                test_pack("new-pack", "1.0.0"),
            ],
        );
        let (remove, add) = compute_diff(&current, &target);
        assert_eq!(remove, vec!["old-pack"]);
        assert_eq!(add.len(), 1);
        assert_eq!(add[0].name, "new-pack");
    }
}
