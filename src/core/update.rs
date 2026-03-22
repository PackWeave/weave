//! Core update orchestration — version comparison + upgrade + apply flow.
//!
//! All business logic lives here; the CLI handler is a thin wrapper that
//! parses arguments, calls these functions, and formats output.

use crate::adapters::{ApplyOptions, CliAdapter};
use crate::core::config::Config;
use crate::core::install::{apply_to_adapters, check_missing_env_vars, MissingEnvVar};
use crate::core::lockfile::LockFile;
use crate::core::pack::{Pack, PackSource, ResolvedPack};
use crate::core::profile::{InstalledPack, Profile};
use crate::core::registry::Registry;
use crate::core::resolver::Resolver;
use crate::core::store::Store;

/// Result for a single pack update.
#[derive(Debug)]
pub struct PackUpdateResult {
    pub name: String,
    pub version: semver::Version,
    pub is_upgrade: bool,
    pub applied_adapters: Vec<String>,
    pub adapter_errors: Vec<String>,
    pub missing_env_vars: Vec<MissingEnvVar>,
}

/// Overall result of an update operation.
#[derive(Debug)]
pub struct UpdateResult {
    /// Packs that were already up to date.
    pub already_up_to_date: Vec<String>,
    /// Packs that were skipped (e.g. local sources).
    pub skipped: Vec<SkippedPack>,
    /// Per-pack results for packs that were updated.
    pub updated: Vec<PackUpdateResult>,
    /// Whether any state was actually modified (for save decisions).
    pub any_updated: bool,
}

/// A pack that was skipped during update.
#[derive(Debug)]
pub struct SkippedPack {
    pub name: String,
    pub reason: String,
}

/// Parse a pack spec like "foo" or "foo@latest" into (name, optional version req).
///
/// - "foo" -> ("foo", None) — caller derives ^major constraint from installed version
/// - "foo@latest" -> ("foo", None) — no constraint, get absolute latest
/// - "@foo" -> ("foo", None) — leading @ stripped
/// - "foo@^2.0" -> ("foo", Some(^2.0))
pub fn parse_pack_spec(
    spec: &str,
) -> std::result::Result<(String, Option<semver::VersionReq>), anyhow::Error> {
    use anyhow::Context;

    if let Some((name, suffix)) = spec.rsplit_once('@') {
        if name.is_empty() {
            return Ok((suffix.to_string(), None));
        }
        if suffix == "latest" {
            return Ok((name.to_string(), None));
        }
        let req = semver::VersionReq::parse(suffix)
            .with_context(|| format!("invalid version requirement '{suffix}'"))?;
        Ok((name.to_string(), Some(req)))
    } else {
        Ok((spec.to_string(), None))
    }
}

/// Build a version requirement that stays within the current major version.
///
/// For major >= 1: `^<major>.0.0` (e.g. `^1.0.0` matches `>=1.0.0, <2.0.0`).
/// For major 0: `>=0.0.0, <1.0.0`.
pub fn major_version_req(version: &semver::Version) -> semver::VersionReq {
    let req_str = if version.major == 0 {
        ">=0.0.0, <1.0.0".to_string()
    } else {
        format!("^{}.0.0", version.major)
    };
    semver::VersionReq::parse(&req_str).expect("generated version req is always valid")
}

/// Determine which packs to update and execute the update.
pub fn update_packs(
    pack_spec: Option<&str>,
    config: &Config,
    registry: &dyn Registry,
    profile: &mut Profile,
    lockfile: &mut LockFile,
    adapters: &[Box<dyn CliAdapter>],
) -> std::result::Result<UpdateResult, anyhow::Error> {
    use anyhow::Context;

    // Determine which packs to update and their version constraints.
    let packs_to_update: Vec<(String, Option<semver::VersionReq>)> = match pack_spec {
        Some(spec) => {
            let (name, version_req) = parse_pack_spec(spec)?;
            let name = name.strip_prefix('@').unwrap_or(&name).to_string();

            if !profile.has_pack(&name) {
                anyhow::bail!(
                    "'{name}' is not installed. Run `weave install {name}` to install it first."
                );
            }
            vec![(name, version_req)]
        }
        None => {
            if profile.packs.is_empty() {
                return Ok(UpdateResult {
                    already_up_to_date: vec![],
                    skipped: vec![],
                    updated: vec![],
                    any_updated: false,
                });
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

    let resolver = Resolver::new(registry);
    let mut result = UpdateResult {
        already_up_to_date: vec![],
        skipped: vec![],
        updated: vec![],
        any_updated: false,
    };

    for (name, version_req) in &packs_to_update {
        // Local packs are not updated automatically.
        if let Some(locked) = lockfile.packs.get(name) {
            if matches!(&locked.source, Some(PackSource::Local { .. })) {
                result.skipped.push(SkippedPack {
                    name: name.clone(),
                    reason: "local source; re-run `weave install ./path` to refresh".to_string(),
                });
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

        let plan = resolver.plan_install(name, effective_req.as_ref(), profile)?;

        if plan.to_install.is_empty() {
            result.already_up_to_date.push(name.clone());
            continue;
        }

        for (resolved_name, version) in &plan.to_install {
            let is_upgrade = profile.has_pack(resolved_name);

            // Fetch from registry and store
            let release = registry.fetch_version(resolved_name, version)?;
            let pack_dir = Store::fetch(resolved_name, &release, None)?;

            // Load the pack manifest
            let pack = Pack::load(&pack_dir)?;

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

            let resolved = ResolvedPack {
                pack: pack.clone(),
                source: PackSource::Registry {
                    registry_url: config.registry_url.clone(),
                },
            };

            // Update does not apply hooks by default — the user must pass
            // --allow-hooks on a fresh install or sync to opt in.
            let apply_options = ApplyOptions::default();
            let (applied_adapters, adapter_errors) =
                apply_to_adapters(&resolved, adapters, &apply_options);

            let missing_env_vars = check_missing_env_vars(&pack);

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

            // Mark updated regardless of adapter errors — profile/lockfile state
            // has already been mutated and the store has the new version.
            result.any_updated = true;

            result.updated.push(PackUpdateResult {
                name: resolved_name.clone(),
                version: version.clone(),
                is_upgrade,
                applied_adapters,
                adapter_errors,
                missing_env_vars,
            });
        }
    }

    if result.any_updated {
        profile
            .save()
            .map_err(anyhow::Error::from)
            .context("saving profile")?;
        lockfile
            .save(&config.active_profile)
            .map_err(anyhow::Error::from)
            .context("saving lock file")?;
    }

    Ok(result)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn major_version_req_major_1() {
        let v = semver::Version::new(1, 2, 3);
        let req = major_version_req(&v);
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
        assert!(parse_pack_spec("foo@not-a-version").is_err());
    }
}
