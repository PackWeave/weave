use anyhow::{Context, Result};

use crate::adapters::{self, ApplyOptions};
use crate::cli::style;
use crate::core::config::Config;
use crate::core::profile::Profile;
use crate::core::registry::registry_from_config;
use crate::core::use_profile;
use crate::error::WeaveError;

/// Switch to a named profile, or print the active profile if no name is given.
/// When `allow_hooks` is true, hooks declared in pack manifests are applied.
/// When `dry_run` is true, preview the profile switch without writing.
pub fn run(profile_name: Option<&str>, allow_hooks: bool, dry_run: bool) -> Result<()> {
    let mut config = Config::load().context("loading weave config")?;

    // If no profile name given, just print the current active profile.
    let target_name = match profile_name {
        Some(name) => name,
        None => {
            println!("{}", style::emphasis(config.active_profile.as_str()));
            return Ok(());
        }
    };

    // If already on this profile, nothing to do.
    if config.active_profile == target_name {
        println!("Already on profile '{}'", style::emphasis(target_name));
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

    let installed_adapters = adapters::installed_adapters();
    let apply_options = ApplyOptions { allow_hooks };
    let registry = registry_from_config(&config);

    let result = use_profile::switch(
        target_name,
        &mut config,
        &current_profile,
        &target_profile,
        &installed_adapters,
        &apply_options,
        &registry,
        dry_run,
    )?;

    if dry_run {
        println!("{}", style::header("Dry run — no changes will be made:"));
        println!();
        for remove_result in &result.removed {
            let adapter_names: Vec<_> = remove_result
                .removed_adapters
                .iter()
                .map(|a| style::target(a.as_str()).to_string())
                .collect();
            println!(
                "  Would remove {} from {}",
                style::pack_name(remove_result.pack_name.as_str()),
                adapter_names.join(", ")
            );
        }
        for apply_result in &result.applied {
            if let Some(err) = &apply_result.load_error {
                eprintln!("  {}: {err}", style::dim("warning"));
                continue;
            }
            let adapter_names: Vec<_> = apply_result
                .applied_adapters
                .iter()
                .map(|a| style::target(a.as_str()).to_string())
                .collect();
            println!(
                "  Would apply {}@{} to {}",
                style::pack_name(apply_result.name.as_str()),
                style::version(apply_result.version.to_string()),
                adapter_names.join(", ")
            );
        }
        println!(
            "  Would switch to profile '{}'",
            style::emphasis(target_name)
        );
        return Ok(());
    }

    // Format output for removals
    for remove_result in &result.removed {
        println!(
            "  Removing {}...",
            style::pack_name(remove_result.pack_name.as_str())
        );
        for adapter in &remove_result.removed_adapters {
            println!("    Removed from {}", style::target(adapter.as_str()));
        }
        for w in &remove_result.adapter_warnings {
            eprintln!("  {}: {w}", style::dim("warning"));
        }
        for e in &remove_result.adapter_errors {
            eprintln!("  {}: {e}", style::dim("warning"));
        }
    }

    // Format output for applies
    for apply_result in &result.applied {
        if let Some(err) = &apply_result.load_error {
            eprintln!("  {}: {err}", style::dim("warning"));
            continue;
        }
        println!(
            "  Applying {}@{}...",
            style::pack_name(apply_result.name.as_str()),
            style::version(apply_result.version.to_string())
        );
        for adapter in &apply_result.applied_adapters {
            println!(
                "    {} to {}",
                style::success("Applied"),
                style::target(adapter.as_str())
            );
        }
        for e in &apply_result.adapter_errors {
            eprintln!("  {}: {e}", style::dim("warning"));
        }
    }

    println!("Switched to profile '{}'", style::emphasis(target_name));
    Ok(())
}
