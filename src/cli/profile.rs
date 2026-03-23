use anyhow::{Context, Result, bail};

use crate::adapters::{self, ApplyOptions};
use crate::cli::style;
use crate::core::config::Config;
use crate::core::lockfile::LockFile;
use crate::core::pack::{PackSource, ResolvedPack};
use crate::core::profile::{InstalledPack, Profile};
use crate::core::registry::{Registry, registry_from_config};
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
    println!("Created profile '{}'", style::emphasis(name));
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
    println!("Deleted profile '{}'", style::emphasis(name));
    Ok(())
}

/// List all profiles, marking the active one.
pub fn list() -> Result<()> {
    let config = Config::load().context("loading weave config")?;
    let profiles = Profile::list_all().context("listing profiles")?;

    let mut all_names = profiles;

    // Ensure "default" always appears even if no file exists on disk
    if !all_names.contains(&"default".to_string()) {
        all_names.push("default".to_string());
    }

    // Ensure the active profile always appears even if no file exists on disk
    if !all_names.contains(&config.active_profile) {
        all_names.push(config.active_profile.clone());
    }

    all_names.sort();

    for name in &all_names {
        if *name == config.active_profile {
            println!(
                "* {} {}",
                style::emphasis(name.as_str()),
                style::success("(active)")
            );
        } else {
            println!("  {}", style::subtext(name.as_str()));
        }
    }

    Ok(())
}

/// Add a pack reference to a named profile.
pub fn add_pack(pack_name: &str, profile_name: &str) -> Result<()> {
    let pack_name = pack_name.strip_prefix('@').unwrap_or(pack_name);

    if !Profile::exists(profile_name).context("checking profile existence")? {
        bail!(
            "profile '{profile_name}' does not exist — create it first with `weave profile create {profile_name}`"
        );
    }

    let config = Config::load().context("loading weave config")?;
    let registry = registry_from_config(&config);

    let mut profile = Profile::load(profile_name).context("loading profile")?;

    // Use the resolver to find the latest version
    let resolver = Resolver::new(&registry);
    let plan = resolver.plan_install(pack_name, None, &profile)?;

    if plan.to_install.is_empty() {
        for name in &plan.already_satisfied {
            println!(
                "  {} is already in profile '{}'",
                style::pack_name(name.as_str()),
                style::emphasis(profile_name)
            );
        }
        return Ok(());
    }

    let mut lockfile = LockFile::load(profile_name).context("loading lock file")?;

    let is_active = config.active_profile == profile_name;
    let installed_adapters = if is_active {
        adapters::installed_adapters()
    } else {
        vec![]
    };

    for (name, version) in &plan.to_install {
        // Ensure the pack is in the store
        let release = registry.fetch_version(name, version)?;
        let pack_dir = Store::fetch(name, &release, None)?;

        let source = PackSource::Registry {
            registry_url: config.registry_url.clone(),
        };

        // If this is the active profile, apply to adapters immediately
        if is_active {
            let pack = crate::core::pack::Pack::load(&pack_dir)?;

            // Validate that the manifest matches what was resolved, matching
            // the tamper check in install.rs.
            anyhow::ensure!(
                pack.name == *name,
                "pack manifest name '{}' does not match resolved name '{name}'; \
                 the archive may be corrupt or tampered",
                pack.name
            );
            anyhow::ensure!(
                pack.version == *version,
                "pack manifest version '{}' does not match resolved version '{version}'; \
                 the archive may be corrupt or tampered",
                pack.version
            );

            let resolved = ResolvedPack {
                pack,
                source: source.clone(),
            };
            // Profile add does not apply hooks by default.
            let apply_options = ApplyOptions::default();
            for adapter in &installed_adapters {
                match adapter.apply(&resolved, &apply_options) {
                    Ok(()) => println!(
                        "    {} to {}",
                        style::success("Applied"),
                        style::target(adapter.name())
                    ),
                    Err(e) => eprintln!("  {}: {}: {e}", style::dim("warning"), adapter.name()),
                }
            }
        }

        lockfile.lock_pack(name, version.clone(), source.clone());

        profile.add_pack(InstalledPack {
            name: name.clone(),
            version: version.clone(),
            source,
        });

        println!(
            "  Added {}@{} to profile '{}'",
            style::pack_name(name.as_str()),
            style::version(version.to_string()),
            style::emphasis(profile_name)
        );
    }

    profile.save().context("saving profile")?;
    lockfile.save(profile_name).context("saving lock file")?;
    println!("{}", style::success("Done."));
    Ok(())
}
