use anyhow::{bail, Context, Result};

use crate::core::config::Config;
use crate::core::pack::PackSource;
use crate::core::profile::{InstalledPack, Profile};
use crate::core::registry::{GitHubRegistry, Registry};
use crate::core::resolver::Resolver;
use crate::core::store::Store;
use crate::error::WeaveError;

/// Create a new empty profile.
pub fn create(name: &str) -> Result<()> {
    if Profile::exists(name).context("checking profile existence")? {
        bail!("profile '{name}' already exists");
    }

    let profile = Profile::load(name).context("creating profile")?;
    profile.save().context("saving new profile")?;
    println!("Created profile '{name}'");
    Ok(())
}

/// Delete a profile (refuses if it is the active profile or the default profile).
pub fn delete(name: &str) -> Result<()> {
    if name == "default" {
        return Err(WeaveError::DefaultProfileDeletion.into());
    }

    let config = Config::load().context("loading weave config")?;
    if config.active_profile == name {
        return Err(WeaveError::ActiveProfileDeletion {
            name: name.to_string(),
        }
        .into());
    }

    Profile::delete(name).context("deleting profile")?;
    println!("Deleted profile '{name}'");
    Ok(())
}

/// List all profiles, marking the active one.
pub fn list() -> Result<()> {
    let config = Config::load().context("loading weave config")?;
    let profiles = Profile::list_all().context("listing profiles")?;

    if profiles.is_empty() {
        // If no profiles saved yet, just show the default as active
        println!("* default (active)");
        return Ok(());
    }

    // Ensure "default" always appears even if no file exists on disk
    let mut all_names = profiles;
    if !all_names.contains(&"default".to_string()) {
        all_names.insert(0, "default".to_string());
    }

    for name in &all_names {
        if *name == config.active_profile {
            println!("* {name} (active)");
        } else {
            println!("  {name}");
        }
    }

    Ok(())
}

/// Add a pack reference to a named profile.
pub fn add_pack(pack_name: &str, profile_name: &str) -> Result<()> {
    let pack_name = pack_name.strip_prefix('@').unwrap_or(pack_name);

    let config = Config::load().context("loading weave config")?;
    let registry = GitHubRegistry::new(&config.registry_url);

    let mut profile = Profile::load(profile_name).context("loading profile")?;

    // Use the resolver to find the latest version
    let resolver = Resolver::new(&registry);
    let plan = resolver.plan_install(pack_name, None, &profile)?;

    if plan.to_install.is_empty() {
        for name in &plan.already_satisfied {
            println!("  {name} is already in profile '{profile_name}'");
        }
        return Ok(());
    }

    for (name, version) in &plan.to_install {
        // Ensure the pack is in the store
        let release = registry.fetch_version(name, version)?;
        let _pack_dir = Store::fetch(name, &release)?;

        profile.add_pack(InstalledPack {
            name: name.clone(),
            version: version.clone(),
            source: PackSource::Registry {
                registry_url: config.registry_url.clone(),
            },
        });

        println!("  Added {name}@{version} to profile '{profile_name}'");
    }

    profile.save().context("saving profile")?;
    println!("Done.");
    Ok(())
}
