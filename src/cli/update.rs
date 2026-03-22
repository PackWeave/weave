use anyhow::{Context, Result};

use crate::adapters;
use crate::cli::style;
use crate::core::config::Config;
use crate::core::lockfile::LockFile;
use crate::core::profile::Profile;
use crate::core::registry::registry_from_config;
use crate::core::update;

/// Update one or all installed packs to the latest compatible version.
///
/// - `pack_spec` = None -> update all installed packs
/// - `pack_spec` = Some("foo") -> update pack "foo" within current major
/// - `pack_spec` = Some("foo@latest") -> update pack "foo" across major versions
pub fn run(pack_spec: Option<&str>) -> Result<()> {
    let config = Config::load().context("loading weave config")?;
    let registry = registry_from_config(&config);
    let mut profile = Profile::load(&config.active_profile).context("loading active profile")?;
    let mut lockfile = LockFile::load(&config.active_profile).context("loading lock file")?;
    let adapters = adapters::installed_adapters();

    // Handle empty profile early for user-friendly message.
    if pack_spec.is_none() && profile.packs.is_empty() {
        println!(
            "{}",
            style::subtext("No packs installed. Nothing to update.")
        );
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
        println!(
            "  {} '{}' — {}",
            style::dim("skipping"),
            style::pack_name(skipped.name.as_str()),
            style::subtext(skipped.reason.as_str())
        );
    }

    for name in &result.already_up_to_date {
        println!(
            "  {} is already up to date",
            style::pack_name(name.as_str())
        );
    }

    for pack_result in &result.updated {
        if pack_result.is_upgrade {
            println!(
                "  Updating {} to {}...",
                style::pack_name(pack_result.name.as_str()),
                style::version(pack_result.version.to_string())
            );
        } else {
            println!(
                "  Installing dependency {}@{}...",
                style::pack_name(pack_result.name.as_str()),
                style::version(pack_result.version.to_string())
            );
        }

        for adapter in &pack_result.applied_adapters {
            println!(
                "    {} to {}",
                style::success("Applied"),
                style::target(adapter.as_str())
            );
        }
        for err in &pack_result.adapter_errors {
            eprintln!("  {}: {err}", style::dim("warning"));
        }
        for env_var in &pack_result.missing_env_vars {
            eprintln!(
                "  {}: pack '{}' requires {} to be set",
                style::dim("warning"),
                style::pack_name(env_var.pack_name.as_str()),
                style::emphasis(env_var.key.as_str())
            );
            if let Some(desc) = &env_var.description {
                eprintln!("  {}: {desc}", style::dim(env_var.key.as_str()));
            }
            eprintln!("  set it with: export {}=<value>", env_var.key);
        }
    }

    if result.any_updated {
        println!("{}", style::success("Done."));
    }

    Ok(())
}
