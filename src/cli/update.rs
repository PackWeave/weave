use anyhow::{bail, Context, Result};

use crate::adapters;
use crate::core::config::Config;
use crate::core::lockfile::LockFile;
use crate::core::pack::PackSource;
use crate::core::profile::{InstalledPack, Profile};
use crate::core::registry::{GitHubRegistry, Registry};
use crate::core::resolver::Resolver;
use crate::core::store::Store;

/// Update one or all installed packs to the latest compatible version.
///
/// - `pack_spec` = None → update all installed packs
/// - `pack_spec` = Some("foo") → update pack "foo" within current major
/// - `pack_spec` = Some("foo@latest") → update pack "foo" across major versions
pub fn run(pack_spec: Option<&str>) -> Result<()> {
    let config = Config::load().context("loading weave config")?;
    let registry = GitHubRegistry::new(&config.registry_url);
    let mut profile = Profile::load(&config.active_profile).context("loading active profile")?;

    // Determine which packs to update and their version constraints.
    let packs_to_update: Vec<(String, Option<semver::VersionReq>)> = match pack_spec {
        Some(spec) => {
            let (name, version_req) = parse_pack_spec(spec)?;
            let name = name.strip_prefix('@').unwrap_or(&name).to_string();

            if !profile.has_pack(&name) {
                bail!("'{name}' is not installed. Run `weave install {name}` to install it first.");
            }
            vec![(name, version_req)]
        }
        None => {
            if profile.packs.is_empty() {
                println!("No packs installed. Nothing to update.");
                return Ok(());
            }
            profile
                .packs
                .iter()
                .map(|p| {
                    let req = major_version_req(&p.version);
                    (p.name.clone(), Some(req))
                })
                .collect()
        }
    };

    let resolver = Resolver::new(&registry);
    let mut lockfile = LockFile::load(&config.active_profile).context("loading lock file")?;
    let adapters = adapters::installed_adapters();

    let mut any_updated = false;

    for (name, version_req) in &packs_to_update {
        // Local packs are not updated automatically — the user re-runs
        // `weave install ./path` to refresh them.
        if let Some(locked) = lockfile.packs.get(name) {
            if matches!(&locked.source, Some(PackSource::Local { .. })) {
                println!(
                    "  skipping '{name}' — local source; re-run `weave install ./path` to refresh"
                );
                continue;
            }
        }

        // For a named pack without @latest, derive the major-version constraint
        // from whatever is currently installed.
        let effective_req = match version_req {
            Some(req) => Some(req.clone()),
            None => {
                let installed = profile.get_pack(name).expect("pack presence checked above");
                Some(major_version_req(&installed.version))
            }
        };

        let plan = resolver.plan_install(name, effective_req.as_ref(), &profile)?;

        if plan.to_install.is_empty() {
            println!("  {name} is already up to date");
            continue;
        }

        for (resolved_name, version) in &plan.to_install {
            // Determine whether this is an upgrade of an already-installed pack
            // or a new transitive dependency being pulled in.
            let is_upgrade = profile.has_pack(resolved_name);

            if is_upgrade {
                println!("  Updating {resolved_name} to {version}...");
            } else {
                println!("  Installing dependency {resolved_name}@{version}...");
            }

            // Fetch from registry and store
            let release = registry.fetch_version(resolved_name, version)?;
            let pack_dir = Store::fetch(resolved_name, &release)?;

            // Load the pack manifest
            let pack = crate::core::pack::Pack::load(&pack_dir)?;

            // Validate manifest matches resolved metadata
            anyhow::ensure!(
                pack.name == *resolved_name,
                "pack manifest name '{}' does not match resolved name '{resolved_name}'; \
                 the archive may be corrupt or tampered",
                pack.name
            );
            anyhow::ensure!(
                pack.version == *version,
                "pack manifest version '{}' does not match resolved version '{version}'; \
                 the archive may be corrupt or tampered",
                pack.version
            );

            let resolved = crate::core::pack::ResolvedPack {
                pack: pack.clone(),
                source: PackSource::Registry {
                    registry_url: config.registry_url.clone(),
                },
            };

            // Apply new version to each installed adapter (collect-and-continue).
            // apply() is idempotent — it overwrites existing entries for the same
            // server/prompt/command names, so an explicit remove() is unnecessary.
            // Applying first means the old config stays intact if apply() fails.
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

            // Warn about required env vars that are not set.
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
                name: resolved_name.clone(),
                version: version.clone(),
                source: PackSource::Registry {
                    registry_url: config.registry_url.clone(),
                },
            });

            // Record in lock file
            lockfile.lock_pack(
                resolved_name,
                version.clone(),
                PackSource::Registry {
                    registry_url: config.registry_url.clone(),
                },
            );

            if adapter_errors.is_empty() {
                any_updated = true;
            }
        }
    }

    if any_updated {
        // Save state
        profile.save().context("saving profile")?;
        lockfile
            .save(&config.active_profile)
            .context("saving lock file")?;
        println!("Done.");
    }

    Ok(())
}

/// Parse a pack spec like "foo" or "foo@latest" into (name, optional version req).
///
/// - "foo" → ("foo", None) — caller derives ^major constraint from installed version
/// - "foo@latest" → ("foo", None passed as-is but flagged) — no constraint, get absolute latest
///
/// For `@latest`, we return None to signal "no version constraint" which makes
/// the resolver pick the absolute latest version (cross-major upgrade).
fn parse_pack_spec(spec: &str) -> Result<(String, Option<semver::VersionReq>)> {
    if let Some((name, suffix)) = spec.rsplit_once('@') {
        // Handle "foo@latest" → no constraint (cross-major)
        // Handle "@foo" (leading @) → name is "foo", no suffix
        if name.is_empty() {
            // This is "@foo" syntax, not "foo@latest"
            return Ok((suffix.to_string(), None));
        }
        if suffix == "latest" {
            return Ok((name.to_string(), None));
        }
        // Treat as a version requirement: "foo@^1.0"
        let req = semver::VersionReq::parse(suffix)
            .with_context(|| format!("invalid version requirement '{suffix}'"))?;
        Ok((name.to_string(), Some(req)))
    } else {
        // No @ → derive constraint from installed version (caller handles this)
        Ok((spec.to_string(), None))
    }
}

/// Build a version requirement that stays within the current major version.
///
/// For major >= 1: `^<major>.0.0` (e.g. `^1.0.0` matches `>=1.0.0, <2.0.0`).
/// For major 0: `>=0.0.0, <1.0.0` because semver caret on `^0.0.0` is too
/// restrictive (only `0.0.x`). The spec says "stay within current major" which
/// for 0.x means any pre-1.0 release.
fn major_version_req(version: &semver::Version) -> semver::VersionReq {
    let req_str = if version.major == 0 {
        ">=0.0.0, <1.0.0".to_string()
    } else {
        format!("^{}.0.0", version.major)
    };
    // Safe: we control the format string, it's always valid semver.
    semver::VersionReq::parse(&req_str).expect("generated version req is always valid")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn major_version_req_major_1() {
        let v = semver::Version::new(1, 2, 3);
        let req = major_version_req(&v);
        // ^1.0.0 should match 1.x.y but not 2.0.0
        assert!(req.matches(&semver::Version::new(1, 0, 0)));
        assert!(req.matches(&semver::Version::new(1, 5, 0)));
        assert!(req.matches(&semver::Version::new(1, 99, 99)));
        assert!(!req.matches(&semver::Version::new(2, 0, 0)));
        assert!(!req.matches(&semver::Version::new(0, 9, 0)));
    }

    #[test]
    fn major_version_req_major_0() {
        let v = semver::Version::new(0, 3, 1);
        let req = major_version_req(&v);
        // ^0.0.0 matches >=0.0.0, <1.0.0
        assert!(req.matches(&semver::Version::new(0, 0, 0)));
        assert!(req.matches(&semver::Version::new(0, 99, 0)));
        assert!(!req.matches(&semver::Version::new(1, 0, 0)));
    }

    #[test]
    fn parse_pack_spec_plain_name() {
        let (name, req) = parse_pack_spec("webdev").unwrap();
        assert_eq!(name, "webdev");
        assert!(req.is_none());
    }

    #[test]
    fn parse_pack_spec_at_latest() {
        let (name, req) = parse_pack_spec("webdev@latest").unwrap();
        assert_eq!(name, "webdev");
        assert!(req.is_none());
    }

    #[test]
    fn parse_pack_spec_at_prefix() {
        // "@webdev" is the leading-@ normalisation case
        let (name, req) = parse_pack_spec("@webdev").unwrap();
        assert_eq!(name, "webdev");
        assert!(req.is_none());
    }

    #[test]
    fn parse_pack_spec_with_version() {
        let (name, req) = parse_pack_spec("webdev@^2.0").unwrap();
        assert_eq!(name, "webdev");
        let req = req.unwrap();
        assert!(req.matches(&semver::Version::new(2, 1, 0)));
        assert!(!req.matches(&semver::Version::new(3, 0, 0)));
    }

    #[test]
    fn parse_pack_spec_scoped_name_at_latest() {
        // "@my-org/my-pack@latest" — scoped name with @latest suffix.
        // The leading @ is stripped by the caller, not parse_pack_spec.
        let (name, req) = parse_pack_spec("@my-org/my-pack@latest").unwrap();
        assert_eq!(name, "@my-org/my-pack");
        assert!(req.is_none());
    }

    #[test]
    fn parse_pack_spec_hyphenated_name() {
        let (name, req) = parse_pack_spec("my-cool-pack").unwrap();
        assert_eq!(name, "my-cool-pack");
        assert!(req.is_none());
    }

    #[test]
    fn parse_pack_spec_invalid_version_req() {
        // "foo@not-a-version" should fail because "not-a-version" is not valid semver
        assert!(parse_pack_spec("foo@not-a-version").is_err());
    }
}
