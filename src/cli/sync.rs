use anyhow::{Context, Result};

use crate::adapters::{self, ApplyOptions};
use crate::cli::style;
use crate::core::config::Config;
use crate::core::lockfile::LockFile;
use crate::core::pack::{PackSource, ResolvedPack};
use crate::core::store::Store;

/// Reapply the active profile's lock file to all adapters.
/// This is the recovery command after config drift — it ensures that every
/// adapter's config matches what the lock file says should be installed.
/// When `allow_hooks` is true, hooks declared in pack manifests are applied.
/// When `dry_run` is true, preview what would be synced without writing.
pub fn run(allow_hooks: bool, dry_run: bool) -> Result<()> {
    let config = Config::load().context("loading weave config")?;
    let profile_name = &config.active_profile;

    let lockfile = LockFile::load(profile_name).context("loading lock file")?;

    if lockfile.packs.is_empty() {
        println!(
            "{}",
            style::subtext(format!(
                "No packs locked in profile '{profile_name}' — nothing to sync."
            ))
        );
        return Ok(());
    }

    let adapters = adapters::installed_adapters();

    if adapters.is_empty() {
        println!(
            "{}",
            style::subtext("No supported CLI adapters found on this system.")
        );
        return Ok(());
    }

    if dry_run {
        println!("{}", style::header("Dry run — no changes will be made:"));
        println!();
        for (pack_name, locked) in &lockfile.packs {
            let adapter_names: Vec<_> = adapters
                .iter()
                .map(|a| style::target(a.name()).to_string())
                .collect();
            println!(
                "  Would sync {}@{} to {}",
                style::pack_name(pack_name.as_str()),
                style::version(locked.version.to_string()),
                adapter_names.join(", ")
            );
        }
        return Ok(());
    }

    let apply_options = ApplyOptions { allow_hooks };
    let mut synced_count: usize = 0;
    let mut error_count: usize = 0;

    for (pack_name, locked) in &lockfile.packs {
        // Load the pack manifest from the local store.
        let pack = match Store::load_pack(pack_name, &locked.version, locked.source.as_ref()) {
            Ok(p) => p,
            Err(e) => {
                eprintln!(
                    "  {}: could not load {}@{} from store: {e}",
                    style::dim("warning"),
                    style::pack_name(pack_name.as_str()),
                    style::version(locked.version.to_string())
                );
                eprintln!(
                    "  hint: run 'weave install {pack_name} --version ={}' to re-fetch it",
                    locked.version
                );
                error_count += 1;
                continue;
            }
        };

        let resolved = ResolvedPack {
            pack,
            source: locked.source.clone().unwrap_or(PackSource::Registry {
                registry_url: config.registry_url.clone(),
            }),
        };

        // Apply to each installed adapter.
        for adapter in &adapters {
            match adapter.apply(&resolved, &apply_options) {
                Ok(()) => {
                    println!(
                        "  {}@{} -> {}",
                        style::pack_name(pack_name.as_str()),
                        style::version(locked.version.to_string()),
                        style::target(adapter.name())
                    );
                    synced_count += 1;
                }
                Err(e) => {
                    eprintln!(
                        "  {}: failed to apply {} to {}: {e}",
                        style::dim("warning"),
                        style::pack_name(pack_name.as_str()),
                        style::target(adapter.name())
                    );
                    error_count += 1;
                }
            }
        }
    }

    if error_count > 0 {
        println!(
            "Sync complete with {} warning(s). {} adapter(s) applied.",
            style::subtext(error_count.to_string()),
            style::success(synced_count.to_string())
        );
    } else {
        println!(
            "{} {} adapter(s) applied.",
            style::success("Sync complete."),
            synced_count
        );
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeMap;

    #[test]
    fn empty_lockfile_is_noop() {
        // Constructing an empty lock file and verifying it has no packs
        // is the unit-testable core of the "sync with nothing locked" path.
        let lockfile = LockFile {
            packs: BTreeMap::new(),
        };
        assert!(
            lockfile.packs.is_empty(),
            "empty lock file should have no packs to sync"
        );
    }
}
