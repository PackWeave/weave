use anyhow::{Context, Result};

use crate::adapters;
use crate::core::config::Config;
use crate::core::lockfile::LockFile;
use crate::core::profile::Profile;
use crate::core::registry::GitHubRegistry;
use crate::core::resolver::Resolver;

/// Remove an installed pack.
pub fn run(pack_name: &str) -> Result<()> {
    let config = Config::load().context("loading weave config")?;
    let registry = GitHubRegistry::new(&config.registry_url);

    let mut profile = Profile::load(&config.active_profile).context("loading active profile")?;

    // Validate the pack is installed
    let resolver = Resolver::new(&registry);
    let plan = resolver.plan_remove(pack_name, &profile)?;

    // Load lock file
    let mut lockfile = LockFile::load(&config.active_profile).context("loading lock file")?;

    let adapters = adapters::all_adapters();

    for name in &plan.to_remove {
        println!("  Removing {name}...");

        // Remove from each adapter
        for adapter in &adapters {
            if adapter.is_installed() {
                adapter.remove(name)?;
                println!("    Removed from {}", adapter.name());
            }
        }

        // Remove from profile
        profile.remove_pack(name);

        // Remove from lock file
        lockfile.unlock_pack(name);
    }

    // Save state
    profile.save().context("saving profile")?;
    lockfile
        .save(&config.active_profile)
        .context("saving lock file")?;

    println!("Done.");
    Ok(())
}
