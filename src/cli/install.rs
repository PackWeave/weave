use std::collections::HashMap;
use std::path::Path;

use anyhow::{Context, Result};

use crate::adapters;
use crate::core::config::Config;
use crate::core::conflict;
use crate::core::lockfile::LockFile;
use crate::core::pack::{Pack, PackSource, ResolvedPack};
use crate::core::profile::{InstalledPack, Profile};
use crate::core::registry::{GitHubRegistry, PackRelease, Registry};
use crate::core::resolver::Resolver;
use crate::core::store::Store;

/// Install a pack by name (or local path), optionally with a version requirement.
/// When `force` is true, tool-conflict warnings are suppressed.
pub fn run(pack_name: &str, version: Option<&str>, force: bool) -> Result<()> {
    // Local path install — bypasses the registry entirely.
    if is_local_path(pack_name) {
        return install_local(pack_name, force);
    }

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
        lockfile.lock_pack(
            name,
            version.clone(),
            PackSource::Registry {
                registry_url: config.registry_url.clone(),
            },
        );
    }

    // Save state
    profile.save().context("saving profile")?;
    lockfile
        .save(&config.active_profile)
        .context("saving lock file")?;

    println!("Done.");
    Ok(())
}

/// Install a pack from a local directory path (bypasses the registry).
///
/// Reads `pack.toml` and all files from the directory, writes them to the
/// store, and applies the pack to all installed CLI adapters — the same steps
/// as a registry install but without a network fetch.
fn install_local(raw_path: &str, force: bool) -> Result<()> {
    let path = expand_home(raw_path);
    let path = path
        .canonicalize()
        .with_context(|| format!("resolving path '{raw_path}'"))?;

    anyhow::ensure!(
        path.is_dir(),
        "'{raw_path}' is not a directory — local installs require a path to a pack directory containing pack.toml"
    );

    let pack =
        Pack::load(&path).with_context(|| format!("loading pack from '{}'", path.display()))?;

    let name = &pack.name;
    let version = &pack.version;

    // Warn about declared dependencies — they are not auto-resolved for local packs.
    if !pack.dependencies.is_empty() {
        let deps: Vec<_> = pack.dependencies.keys().map(String::as_str).collect();
        eprintln!(
            "  warning: '{name}' declares dependencies: {}",
            deps.join(", ")
        );
        eprintln!("  Install them separately: weave install <pack-name>");
    }

    let config = Config::load().context("loading weave config")?;
    let mut profile = Profile::load(&config.active_profile).context("loading active profile")?;
    let mut lockfile = LockFile::load(&config.active_profile).context("loading lock file")?;

    // Already installed at the same version — nothing to do.
    if let Some(installed) = profile.packs.iter().find(|p| p.name == *name) {
        if installed.version == *version {
            println!("  '{name}@{version}' is already installed");
            return Ok(());
        }
    }

    println!("  Installing {name}@{version} (local)...");

    let files = files_from_dir(&path)
        .with_context(|| format!("reading pack files from '{}'", path.display()))?;

    let release = PackRelease {
        version: version.clone(),
        files,
        dependencies: pack.dependencies.clone(),
    };

    let pack_dir =
        Store::fetch(name, &release).with_context(|| format!("writing pack '{name}' to store"))?;

    // Re-load from store to validate written files.
    let pack = Pack::load(&pack_dir)?;

    let local_source = PackSource::Local {
        path: path.to_string_lossy().to_string(),
    };

    let resolved = ResolvedPack {
        pack: pack.clone(),
        source: local_source.clone(),
    };

    let installed_packs = if !force {
        load_installed_packs(&profile)
    } else {
        vec![]
    };

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

    let adapters = adapters::installed_adapters();
    for adapter in &adapters {
        match adapter.apply(&resolved) {
            Ok(()) => println!("    Applied to {}", adapter.name()),
            Err(e) => eprintln!("  warning: failed to apply to {}: {e}", adapter.name()),
        }
    }

    // Warn about required env vars that are not set.
    for server in &pack.servers {
        for (key, env_var) in &server.env {
            if env_var.required && std::env::var_os(key).is_none() {
                eprintln!("  warning: pack '{name}' requires {key} to be set");
                if let Some(desc) = &env_var.description {
                    eprintln!("  {key}: {desc}");
                }
                eprintln!("  set it with: export {key}=<value>");
            }
        }
    }

    // Remove old version from profile if upgrading.
    profile.remove_pack(name);
    profile.add_pack(InstalledPack {
        name: name.clone(),
        version: version.clone(),
        source: local_source.clone(),
    });
    lockfile.lock_pack(name, version.clone(), local_source);

    profile.save().context("saving profile")?;
    lockfile
        .save(&config.active_profile)
        .context("saving lock file")?;

    println!("Installed {name}@{version} (local)");
    Ok(())
}

/// Return true if `s` looks like a filesystem path rather than a pack name.
/// Pack names are `[a-z0-9-]+`; paths start with `.`, `/`, or `~`.
fn is_local_path(s: &str) -> bool {
    s.starts_with('.') || s.starts_with('/') || s.starts_with('~')
}

/// Expand a leading `~` to the user's home directory.
fn expand_home(s: &str) -> std::path::PathBuf {
    if let Some(rest) = s.strip_prefix("~/") {
        if let Some(home) = dirs::home_dir() {
            return home.join(rest);
        }
    } else if s == "~" {
        if let Some(home) = dirs::home_dir() {
            return home;
        }
    }
    std::path::PathBuf::from(s)
}

/// Walk `dir` recursively and return a flat map of `relative-path → file content`.
/// Skips hidden entries (names starting with `.`).
fn files_from_dir(dir: &Path) -> Result<HashMap<String, String>> {
    let mut files = HashMap::new();
    visit_dir(dir, dir, &mut files)?;
    Ok(files)
}

fn visit_dir(root: &Path, current: &Path, files: &mut HashMap<String, String>) -> Result<()> {
    let entries = std::fs::read_dir(current)
        .with_context(|| format!("reading directory {}", current.display()))?;

    for entry in entries {
        let entry = entry.with_context(|| format!("reading entry in {}", current.display()))?;
        let path = entry.path();

        // Skip hidden files and directories (e.g. .git, .DS_Store).
        if entry.file_name().to_string_lossy().starts_with('.') {
            continue;
        }

        if path.is_dir() {
            visit_dir(root, &path, files)?;
        } else {
            let rel = path
                .strip_prefix(root)
                .expect("path is always under root")
                .to_string_lossy()
                .replace('\\', "/");
            let content = std::fs::read_to_string(&path)
                .with_context(|| format!("reading {}", path.display()))?;
            files.insert(rel, content);
        }
    }
    Ok(())
}

/// Load pack manifests for all currently-installed packs from the local store.
/// Packs that cannot be loaded (e.g. missing from the store) are skipped with a
/// warning — a missing manifest should not block an install, but the user should
/// know about store/profile inconsistencies.
fn load_installed_packs(profile: &Profile) -> Vec<Pack> {
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
