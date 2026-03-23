use anyhow::{Context, Result};

use crate::adapters;
use crate::cli::style;
use crate::core::config::Config;
use crate::core::lockfile::LockFile;
use crate::core::profile::Profile;
use crate::core::registry::registry_from_config;
use crate::core::resolver::Resolver;

/// Remove an installed pack.
pub fn run(pack_name: &str) -> Result<()> {
    // Normalise name: strip a leading '@' so `weave remove @webdev` works.
    let pack_name = pack_name.strip_prefix('@').unwrap_or(pack_name);

    let config = Config::load().context("loading weave config")?;
    let registry = registry_from_config(&config);

    let mut profile = Profile::load(&config.active_profile).context("loading active profile")?;

    // Validate the pack is installed
    let resolver = Resolver::new(&registry);
    let plan = resolver.plan_remove(pack_name, &profile)?;

    // Load lock file
    let mut lockfile = LockFile::load(&config.active_profile).context("loading lock file")?;

    let adapters = adapters::installed_adapters();

    for name in &plan.to_remove {
        println!("  Removing {}...", style::pack_name(name.as_str()));

        // Remove from each installed adapter. Continue through failures so the
        // profile/lockfile are always updated and partial state is surfaced as
        // warnings rather than leaving the pack stuck as "installed".
        let mut adapter_errors: Vec<String> = Vec::new();
        for adapter in &adapters {
            match adapter.remove(name) {
                Ok(warnings) => {
                    println!("    Removed from {}", style::target(adapter.name()));
                    for w in warnings {
                        eprintln!("  {}: {}: {w}", style::dim("warning"), adapter.name());
                    }
                }
                Err(e) => adapter_errors.push(format!("{}: {e}", adapter.name())),
            }
        }
        for err in &adapter_errors {
            eprintln!("  {}: {err}", style::dim("warning"));
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

    println!("{}", style::success("Done."));
    Ok(())
}
