use anyhow::{Context, Result};

use crate::adapters;
use crate::core::config::Config;
use crate::core::lockfile::LockFile;
use crate::core::profile::Profile;
use crate::core::registry::GitHubRegistry;
use crate::core::update;

/// Update one or all installed packs to the latest compatible version.
///
/// - `pack_spec` = None -> update all installed packs
/// - `pack_spec` = Some("foo") -> update pack "foo" within current major
/// - `pack_spec` = Some("foo@latest") -> update pack "foo" across major versions
pub fn run(pack_spec: Option<&str>) -> Result<()> {
    let config = Config::load().context("loading weave config")?;
    let registry = GitHubRegistry::new(&config.registry_url);
    let mut profile = Profile::load(&config.active_profile).context("loading active profile")?;
    let mut lockfile = LockFile::load(&config.active_profile).context("loading lock file")?;
    let adapters = adapters::installed_adapters();

    // Handle empty profile early for user-friendly message.
    if pack_spec.is_none() && profile.packs.is_empty() {
        println!("No packs installed. Nothing to update.");
        return Ok(());
    }

    let result = update::update_packs(
        pack_spec,
        &config,
        &registry,
        &mut profile,
        &mut lockfile,
        &adapters,
    )?;

    // Format output
    for skipped in &result.skipped {
        println!("  skipping '{}' — {}", skipped.name, skipped.reason);
    }

    for name in &result.already_up_to_date {
        println!("  {name} is already up to date");
    }

    for pack_result in &result.updated {
        if pack_result.is_upgrade {
            println!(
                "  Updating {} to {}...",
                pack_result.name, pack_result.version
            );
        } else {
            println!(
                "  Installing dependency {}@{}...",
                pack_result.name, pack_result.version
            );
        }

        for adapter in &pack_result.applied_adapters {
            println!("    Applied to {adapter}");
        }
        for err in &pack_result.adapter_errors {
            eprintln!("  warning: {err}");
        }
        for env_var in &pack_result.missing_env_vars {
            eprintln!(
                "  warning: pack '{}' requires {} to be set",
                env_var.pack_name, env_var.key
            );
            if let Some(desc) = &env_var.description {
                eprintln!("  {}: {desc}", env_var.key);
            }
            eprintln!("  set it with: export {}=<value>", env_var.key);
        }
    }

    if result.any_updated {
        println!("Done.");
    }

    Ok(())
}
