use anyhow::{Context, Result};

use crate::adapters;
use crate::core::config::Config;
use crate::core::lockfile::LockFile;
use crate::core::pack::PackSource;
use crate::core::profile::{InstalledPack, Profile};
use crate::core::registry::{GitHubRegistry, Registry};
use crate::core::resolver::Resolver;
use crate::core::store::Store;

/// Install a pack by name, optionally with a version requirement.
pub fn run(pack_name: &str, version: Option<&str>) -> Result<()> {
    let config = Config::load().context("loading weave config")?;
    let registry = GitHubRegistry::new(&config.registry_url);

    let version_req = match version {
        Some(v) => Some(
            semver::VersionReq::parse(v)
                .with_context(|| format!("invalid version requirement '{v}'"))?,
        ),
        None => None,
    };

    let mut profile = Profile::load(&config.active_profile).context("loading active profile")?;

    // Resolve dependencies and build install plan
    let resolver = Resolver::new(&registry);
    let plan = resolver.plan_install(pack_name, version_req.as_ref(), &profile)?;

    if plan.to_install.is_empty() {
        for name in &plan.already_satisfied {
            println!("  {name} is already installed and up to date");
        }
        return Ok(());
    }

    // Load lock file
    let mut lockfile = LockFile::load(&config.active_profile).context("loading lock file")?;

    // Get installed adapters
    let adapters = adapters::all_adapters();

    for (name, version) in &plan.to_install {
        println!("  Installing {name}@{version}...");

        // Fetch from registry and store
        let release = registry.fetch_version(name, version)?;
        let pack_dir = Store::fetch(name, &release)?;

        // Load the pack manifest
        let pack = crate::core::pack::Pack::load(&pack_dir)?;

        let resolved = crate::core::pack::ResolvedPack {
            pack: pack.clone(),
            source: PackSource::Registry {
                registry_url: config.registry_url.clone(),
            },
        };

        // Apply to each adapter
        for adapter in &adapters {
            if adapter.is_installed() {
                adapter.apply(&resolved)?;
                println!("    Applied to {}", adapter.name());
            }
        }

        // Record in profile
        profile.add_pack(InstalledPack {
            name: name.clone(),
            version: version.clone(),
            source: PackSource::Registry {
                registry_url: config.registry_url.clone(),
            },
        });

        // Record in lock file
        lockfile.lock_pack(name, version.clone(), Some(release.sha256.clone()));
    }

    // Save state
    profile.save().context("saving profile")?;
    lockfile
        .save(&config.active_profile)
        .context("saving lock file")?;

    println!("Done.");
    Ok(())
}
