use anyhow::{Context, Result};

use crate::adapters::{self, ApplyOptions};
use crate::core::config::Config;
use crate::core::profile::Profile;
use crate::core::use_profile;
use crate::error::WeaveError;

/// Switch to a named profile, or print the active profile if no name is given.
/// When `allow_hooks` is true, hooks declared in pack manifests are applied.
pub fn run(profile_name: Option<&str>, allow_hooks: bool) -> Result<()> {
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

    let installed_adapters = adapters::installed_adapters();
    let apply_options = ApplyOptions { allow_hooks };

    let result = use_profile::switch(
        target_name,
        &mut config,
        &current_profile,
        &target_profile,
        &installed_adapters,
        &apply_options,
    )?;

    // Format output for removals
    for remove_result in &result.removed {
        println!("  Removing {}...", remove_result.pack_name);
        for adapter in &remove_result.removed_adapters {
            println!("    Removed from {adapter}");
        }
        for w in &remove_result.adapter_warnings {
            eprintln!("  warning: {w}");
        }
        for e in &remove_result.adapter_errors {
            eprintln!("  warning: {e}");
        }
    }

    // Format output for applies
    for apply_result in &result.applied {
        if let Some(err) = &apply_result.load_error {
            eprintln!("  warning: {err}");
            continue;
        }
        println!(
            "  Applying {}@{}...",
            apply_result.name, apply_result.version
        );
        for adapter in &apply_result.applied_adapters {
            println!("    Applied to {adapter}");
        }
        for e in &apply_result.adapter_errors {
            eprintln!("  warning: {e}");
        }
    }

    println!("Switched to profile '{target_name}'");
    Ok(())
}
