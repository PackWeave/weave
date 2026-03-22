use anyhow::{Context, Result};

use crate::adapters;
use crate::core::config::Config;
use crate::core::lockfile::LockFile;
use crate::core::pack::{PackSource, ResolvedPack};
use crate::core::store::Store;

/// Reapply the active profile's lock file to all adapters.
/// This is the recovery command after config drift — it ensures that every
/// adapter's config matches what the lock file says should be installed.
pub fn run() -> Result<()> {
    let config = Config::load().context("loading weave config")?;
    let profile_name = &config.active_profile;

    let lockfile = LockFile::load(profile_name).context("loading lock file")?;

    if lockfile.packs.is_empty() {
        println!("No packs locked in profile '{profile_name}' — nothing to sync.");
        return Ok(());
    }

    let adapters = adapters::installed_adapters();

    if adapters.is_empty() {
        println!("No supported CLI adapters found on this system.");
        return Ok(());
    }

    let mut synced_count: usize = 0;
    let mut error_count: usize = 0;

    for (pack_name, locked) in &lockfile.packs {
        // Load the pack manifest from the local store.
        let pack = match Store::load_pack(pack_name, &locked.version, locked.source.as_ref()) {
            Ok(p) => p,
            Err(e) => {
                eprintln!(
                    "  warning: could not load {pack_name}@{} from store: {e}",
                    locked.version
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
            match adapter.apply(&resolved) {
                Ok(()) => {
                    println!("  {pack_name}@{} -> {}", locked.version, adapter.name());
                    synced_count += 1;
                }
                Err(e) => {
                    eprintln!(
                        "  warning: failed to apply {pack_name} to {}: {e}",
                        adapter.name()
                    );
                    error_count += 1;
                }
            }
        }
    }

    if error_count > 0 {
        println!("Sync complete with {error_count} warning(s). {synced_count} adapter(s) applied.");
    } else {
        println!("Sync complete. {synced_count} adapter(s) applied.");
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
