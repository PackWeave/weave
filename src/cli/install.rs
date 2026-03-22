use anyhow::{Context, Result};

use crate::adapters;
use crate::core::config::Config;
use crate::core::conflict;
use crate::core::lockfile::LockFile;
use crate::core::pack::PackSource;
use crate::core::profile::{InstalledPack, Profile};
use crate::core::registry::{GitHubRegistry, Registry};
use crate::core::resolver::Resolver;
use crate::core::store::Store;

/// Install a pack by name, optionally with a version requirement.
/// When `force` is true, tool-conflict warnings are suppressed.
pub fn run(pack_name: &str, version: Option<&str>, force: bool) -> Result<()> {
    // Normalise name: strip a leading '@' so `weave install @webdev` works like
    // `weave install webdev` (consistent with how packs are validated/stored).
    let pack_name = pack_name.strip_prefix('@').unwrap_or(pack_name);

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

    let adapters = adapters::installed_adapters();

    // Load installed pack manifests once before the loop rather than on each
    // iteration — avoids redundant I/O when installing multiple packs.
    let installed_packs = if !force {
        load_installed_packs(&profile)
    } else {
        vec![]
    };

    for (name, version) in &plan.to_install {
        println!("  Installing {name}@{version}...");

        // Fetch from registry and store
        let release = registry.fetch_version(name, version)?;
        let pack_dir = Store::fetch(name, &release)?;

        // Load the pack manifest
        let pack = crate::core::pack::Pack::load(&pack_dir)?;

        // Validate that the manifest matches what was resolved. A tampered or
        // mis-labelled archive could contain a pack.toml with a different name or
        // version, causing the wrong adapter manifest entries to be written.
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

        // Check for tool-name conflicts with already-installed packs.
        // This is informational only — it never blocks the install.
        if !force {
            let conflicts = conflict::check_tool_conflicts(&pack, &installed_packs);
            for c in &conflicts {
                eprintln!(
                    "  warning: tool conflict: '{}' is exported by both {}/{} and {}/{}",
                    c.tool_name,
                    c.installed_pack,
                    c.installed_server,
                    c.incoming_pack,
                    c.incoming_server,
                );
            }
        }

        let resolved = crate::core::pack::ResolvedPack {
            pack: pack.clone(),
            source: PackSource::Registry {
                registry_url: config.registry_url.clone(),
            },
        };

        // Apply to each installed adapter. Continue even if one fails so that the
        // pack is still recorded in the profile/lockfile and partial state is
        // surfaced as warnings rather than leaving the install untracked.
        let mut adapter_errors: Vec<String> = Vec::new();
        for adapter in &adapters {
            match adapter.apply(&resolved) {
                Ok(()) => println!("    Applied to {}", adapter.name()),
                Err(e) => adapter_errors.push(format!("{}: {e}", adapter.name())),
            }
        }
        for err in &adapter_errors {
            eprintln!("  warning: {err}");
        }

        // Warn about required env vars that are not set in the current environment.
        // Uses var_os (not var) to avoid a false positive when the var is set to
        // a non-UTF-8 byte sequence — var() would return Err(NotUnicode) even
        // though the variable IS set.
        for server in &pack.servers {
            for (key, env_var) in &server.env {
                if env_var.required && std::env::var_os(key).is_none() {
                    eprintln!("  warning: pack '{}' requires {key} to be set", pack.name);
                    if let Some(desc) = &env_var.description {
                        eprintln!("  {key}: {desc}");
                    }
                    eprintln!("  set it with: export {key}=<value>");
                }
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
        lockfile.lock_pack(name, version.clone(), None);
    }

    // Save state
    profile.save().context("saving profile")?;
    lockfile
        .save(&config.active_profile)
        .context("saving lock file")?;

    println!("Done.");
    Ok(())
}

/// Load pack manifests for all currently-installed packs from the local store.
/// Packs that cannot be loaded (e.g. missing from the store) are skipped with a
/// warning — a missing manifest should not block an install, but the user should
/// know about store/profile inconsistencies.
fn load_installed_packs(profile: &Profile) -> Vec<crate::core::pack::Pack> {
    let mut packs = Vec::new();
    for installed in &profile.packs {
        match Store::load_pack(&installed.name, &installed.version) {
            Ok(pack) => packs.push(pack),
            Err(e) => {
                log::warn!(
                    "could not load manifest for {} v{}: {e}",
                    installed.name,
                    installed.version
                );
            }
        }
    }
    packs
}
