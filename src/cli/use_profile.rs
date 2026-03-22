use anyhow::{bail, Context, Result};

use crate::adapters;
use crate::core::config::Config;
use crate::core::pack::{Pack, PackSource, ResolvedPack};
use crate::core::profile::Profile;
use crate::core::registry::{GitHubRegistry, Registry};
use crate::core::store::Store;
use crate::error::WeaveError;

/// Switch to a named profile, or print the active profile if no name is given.
pub fn run(profile_name: Option<&str>) -> Result<()> {
    let mut config = Config::load().context("loading weave config")?;

    // If no profile name given, just print the current active profile.
    let target_name = match profile_name {
        Some(name) => name,
        None => {
            println!("{}", config.active_profile);
            return Ok(());
        }
    };

    // If already on this profile, nothing to do.
    if config.active_profile == target_name {
        println!("Already on profile '{target_name}'");
        return Ok(());
    }

    // Load target profile — must exist on disk (except "default", which is always valid).
    if target_name != "default"
        && !Profile::exists(target_name).context("checking target profile")?
    {
        return Err(WeaveError::ProfileNotFound {
            name: target_name.to_string(),
        }
        .into());
    }
    let target_profile = Profile::load(target_name).context("loading target profile")?;
    let current_profile =
        Profile::load(&config.active_profile).context("loading current profile")?;

    // Compute the diff
    let (to_remove, to_add) = compute_diff(&current_profile, &target_profile);

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

    let installed_adapters = adapters::installed_adapters();

    // Remove packs that are in current but not in target
    for pack_name in &to_remove {
        println!("  Removing {pack_name}...");
        for adapter in &installed_adapters {
            match adapter.remove(pack_name) {
                Ok(warnings) => {
                    println!("    Removed from {}", adapter.name());
                    for w in warnings {
                        eprintln!("  warning: {}: {w}", adapter.name());
                    }
                }
                Err(e) => eprintln!("  warning: {}: {e}", adapter.name()),
            }
        }
    }

    // Apply packs that are in target but not in current
    for installed in &to_add {
        println!("  Applying {}@{}...", installed.name, installed.version);

        let pack = match load_or_fetch_pack(&installed.name, &installed.version, &installed.source)
        {
            Ok(p) => p,
            Err(e) => {
                eprintln!(
                    "  warning: could not load {}@{}: {e}",
                    installed.name, installed.version
                );
                continue;
            }
        };

        let resolved = ResolvedPack {
            pack,
            source: installed.source.clone(),
        };

        for adapter in &installed_adapters {
            match adapter.apply(&resolved) {
                Ok(()) => println!("    Applied to {}", adapter.name()),
                Err(e) => eprintln!("  warning: {}: {e}", adapter.name()),
            }
        }
    }

    // Update the active profile in config
    config.active_profile = target_name.to_string();
    config.save().context("saving config")?;

    println!("Switched to profile '{target_name}'");
    Ok(())
}

/// Try to load a pack from the store; if missing, attempt to fetch it from the registry.
fn load_or_fetch_pack(name: &str, version: &semver::Version, source: &PackSource) -> Result<Pack> {
    // Try loading from store first
    if let Ok(pack) = Store::load_pack(name, version, Some(source)) {
        return Ok(pack);
    }

    // Try fetching from registry
    let registry_url = match source {
        PackSource::Registry { registry_url } => registry_url,
        _ => bail!("pack {name}@{version} not in local store and source is not a registry"),
    };

    println!("  Fetching {name}@{version} from registry...");
    let registry = GitHubRegistry::new(registry_url);
    let release = registry
        .fetch_version(name, version)
        .context("resolving pack from registry")?;
    Store::fetch(name, &release, Some(source)).context("downloading pack")?;
    Store::load_pack(name, version, Some(source)).context("loading fetched pack")
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
pub fn compute_diff(
    current: &Profile,
    target: &Profile,
) -> (Vec<String>, Vec<crate::core::profile::InstalledPack>) {
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
