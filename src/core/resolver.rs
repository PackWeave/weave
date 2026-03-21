use std::collections::HashSet;

use crate::core::profile::Profile;
use crate::core::registry::Registry;
use crate::error::{Result, WeaveError};

/// The result of dependency resolution: what to install, what to remove,
/// and what is already satisfied.
#[derive(Debug, Clone)]
pub struct InstallPlan {
    pub to_install: Vec<(String, semver::Version)>,
    pub to_remove: Vec<String>,
    pub already_satisfied: Vec<String>,
}

/// Resolves pack dependencies and produces an install plan.
pub struct Resolver<'a> {
    registry: &'a dyn Registry,
}

impl<'a> Resolver<'a> {
    pub fn new(registry: &'a dyn Registry) -> Self {
        Self { registry }
    }

    /// Plan installation of a pack (and its dependencies) into a profile.
    pub fn plan_install(
        &self,
        pack_name: &str,
        version_req: Option<&semver::VersionReq>,
        profile: &Profile,
    ) -> Result<InstallPlan> {
        let mut to_install = Vec::new();
        let mut already_satisfied = Vec::new();
        let mut visited = HashSet::new();

        self.resolve_pack(
            pack_name,
            version_req,
            profile,
            &mut to_install,
            &mut already_satisfied,
            &mut visited,
        )?;

        Ok(InstallPlan {
            to_install,
            to_remove: Vec::new(),
            already_satisfied,
        })
    }

    /// Plan removal of a pack from a profile.
    pub fn plan_remove(&self, pack_name: &str, profile: &Profile) -> Result<InstallPlan> {
        if !profile.has_pack(pack_name) {
            return Err(WeaveError::NotInstalled {
                name: pack_name.to_string(),
            });
        }

        Ok(InstallPlan {
            to_install: Vec::new(),
            to_remove: vec![pack_name.to_string()],
            already_satisfied: Vec::new(),
        })
    }

    fn resolve_pack(
        &self,
        pack_name: &str,
        version_req: Option<&semver::VersionReq>,
        profile: &Profile,
        to_install: &mut Vec<(String, semver::Version)>,
        already_satisfied: &mut Vec<String>,
        visited: &mut HashSet<String>,
    ) -> Result<()> {
        // Cycle detection must come first: `visited` tracks the active resolution
        // stack. If this pack is currently being resolved upstream it means we
        // have a circular dependency (A → B → A). A pack that has already been
        // fully resolved (in `to_install` but not in `visited`) is a diamond
        // dependency and is fine — the duplicate-queue guard below handles it.
        if !visited.insert(pack_name.to_string()) {
            let chain = {
                let mut v: Vec<&str> = visited.iter().map(String::as_str).collect();
                v.sort();
                v.join(" → ")
            };
            return Err(WeaveError::CircularDependency {
                pack: pack_name.to_string(),
                chain,
            });
        }

        // Skip if already queued for installation — this pack was fully resolved
        // in a sibling branch (diamond dependency). Remove from `visited` first
        // because we inserted above and won't recurse further.
        if to_install.iter().any(|(n, _)| n == pack_name) {
            visited.remove(pack_name);
            return Ok(());
        }

        let metadata = self.registry.fetch_metadata(pack_name)?;

        // Find the best matching version
        let version = if let Some(req) = version_req {
            metadata
                .versions
                .iter()
                .filter(|v| req.matches(&v.version))
                .map(|v| &v.version)
                .max()
                .cloned()
                .ok_or_else(|| WeaveError::VersionNotFound {
                    name: pack_name.to_string(),
                    version: req.to_string(),
                    available: metadata
                        .versions
                        .iter()
                        .map(|v| v.version.to_string())
                        .collect::<Vec<_>>()
                        .join(", "),
                })?
        } else {
            metadata.latest_version()?
        };

        // Check if the exact selected version is already installed.
        // We compare against `version` (the best matching release) rather than
        // just checking whether the installed version satisfies the requirement,
        // so that `weave install foo ^1.0` still upgrades from 1.0.0 → 1.1.0
        // when 1.1.0 is the latest release matching the constraint.
        if let Some(installed) = profile.get_pack(pack_name) {
            if installed.version == version {
                already_satisfied.push(pack_name.to_string());
                // Backtrack before returning: this pack is satisfied so we don't
                // traverse its deps again, but it must not stay in `visited` as
                // a false cycle anchor for sibling packs that depend on it.
                visited.remove(pack_name);
                return Ok(());
            }
        }

        to_install.push((pack_name.to_string(), version.clone()));

        // Fetch the release record to get its declared dependencies, then
        // recursively resolve each one.
        let release = self.registry.fetch_version(pack_name, &version)?;
        for (dep_name, dep_req) in &release.dependencies {
            self.resolve_pack(
                dep_name,
                Some(dep_req),
                profile,
                to_install,
                already_satisfied,
                visited,
            )?;
        }

        // Backtrack: remove from the active path so sibling packs that share
        // this dependency don't incorrectly detect a cycle.
        visited.remove(pack_name);

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;

    use super::*;
    use crate::core::pack::PackSource;
    use crate::core::profile::InstalledPack;
    use crate::core::registry::{MockRegistry, PackMetadata, PackRelease};

    /// Build a `PackRelease` with no dependencies.
    fn release(major: u64, minor: u64, patch: u64) -> PackRelease {
        PackRelease {
            version: semver::Version::new(major, minor, patch),
            url: format!("https://example.com/pack-{major}.{minor}.{patch}.tar.gz"),
            sha256: format!("{major}{minor}{patch}"),
            size_bytes: None,
            dependencies: HashMap::new(),
        }
    }

    /// Build a `PackRelease` with the given dependencies.
    fn release_with_deps(major: u64, minor: u64, patch: u64, deps: &[(&str, &str)]) -> PackRelease {
        let dependencies = deps
            .iter()
            .map(|(name, req)| {
                (
                    name.to_string(),
                    // Safe: test strings are valid semver requirements
                    semver::VersionReq::parse(req).unwrap(),
                )
            })
            .collect();
        PackRelease {
            version: semver::Version::new(major, minor, patch),
            url: format!("https://example.com/pack-{major}.{minor}.{patch}.tar.gz"),
            sha256: format!("{major}{minor}{patch}"),
            size_bytes: None,
            dependencies,
        }
    }

    fn pack_meta(name: &str, releases: Vec<PackRelease>) -> PackMetadata {
        PackMetadata {
            name: name.into(),
            description: format!("{name} pack"),
            authors: vec![],
            license: None,
            repository: None,
            versions: releases,
        }
    }

    fn setup_registry() -> MockRegistry {
        let mut registry = MockRegistry::new();
        registry.add_pack(pack_meta(
            "webdev",
            vec![release(1, 0, 0), release(1, 1, 0)],
        ));
        registry
    }

    // ── original tests ────────────────────────────────────────────────────────

    #[test]
    fn plan_install_latest() {
        let registry = setup_registry();
        let resolver = Resolver::new(&registry);
        let profile = Profile {
            name: "test".into(),
            packs: vec![],
        };

        let plan = resolver.plan_install("webdev", None, &profile).unwrap();
        assert_eq!(plan.to_install.len(), 1);
        assert_eq!(plan.to_install[0].0, "webdev");
        assert_eq!(plan.to_install[0].1, semver::Version::new(1, 1, 0));
    }

    #[test]
    fn plan_install_with_version_req() {
        let registry = setup_registry();
        let resolver = Resolver::new(&registry);
        let profile = Profile {
            name: "test".into(),
            packs: vec![],
        };

        let req = semver::VersionReq::parse("^1.0.0").unwrap();
        let plan = resolver
            .plan_install("webdev", Some(&req), &profile)
            .unwrap();
        assert_eq!(plan.to_install[0].1, semver::Version::new(1, 1, 0));
    }

    #[test]
    fn plan_install_already_satisfied() {
        let registry = setup_registry();
        let resolver = Resolver::new(&registry);
        let profile = Profile {
            name: "test".into(),
            packs: vec![InstalledPack {
                name: "webdev".into(),
                version: semver::Version::new(1, 1, 0),
                source: PackSource::Registry {
                    registry_url: "https://example.com".into(),
                },
            }],
        };

        let plan = resolver.plan_install("webdev", None, &profile).unwrap();
        assert!(plan.to_install.is_empty());
        assert_eq!(plan.already_satisfied, vec!["webdev"]);
    }

    #[test]
    fn plan_install_not_found() {
        let registry = setup_registry();
        let resolver = Resolver::new(&registry);
        let profile = Profile {
            name: "test".into(),
            packs: vec![],
        };

        let result = resolver.plan_install("nonexistent", None, &profile);
        assert!(result.is_err());
    }

    #[test]
    fn plan_remove() {
        let registry = setup_registry();
        let resolver = Resolver::new(&registry);
        let profile = Profile {
            name: "test".into(),
            packs: vec![InstalledPack {
                name: "webdev".into(),
                version: semver::Version::new(1, 0, 0),
                source: PackSource::Registry {
                    registry_url: "https://example.com".into(),
                },
            }],
        };

        let plan = resolver.plan_remove("webdev", &profile).unwrap();
        assert_eq!(plan.to_remove, vec!["webdev"]);
    }

    #[test]
    fn plan_install_upgrades_within_range() {
        // Installed 1.0.0, req ^1.0.0, latest is 1.1.0 → should plan an upgrade
        let registry = setup_registry();
        let resolver = Resolver::new(&registry);
        let profile = Profile {
            name: "test".into(),
            packs: vec![InstalledPack {
                name: "webdev".into(),
                version: semver::Version::new(1, 0, 0),
                source: PackSource::Registry {
                    registry_url: "https://example.com".into(),
                },
            }],
        };

        let req = semver::VersionReq::parse("^1.0.0").unwrap();
        let plan = resolver
            .plan_install("webdev", Some(&req), &profile)
            .unwrap();
        assert_eq!(
            plan.to_install[0].1,
            semver::Version::new(1, 1, 0),
            "should plan upgrade to 1.1.0"
        );
        assert!(plan.already_satisfied.is_empty());
    }

    #[test]
    fn plan_remove_not_installed() {
        let registry = setup_registry();
        let resolver = Resolver::new(&registry);
        let profile = Profile {
            name: "test".into(),
            packs: vec![],
        };

        let result = resolver.plan_remove("webdev", &profile);
        assert!(result.is_err());
    }

    // ── new transitive dependency tests ───────────────────────────────────────

    /// Installing pack A (which depends on pack B) should also queue pack B.
    #[test]
    fn transitive_dependency_installed() {
        let mut registry = MockRegistry::new();
        registry.add_pack(pack_meta("pack-b", vec![release(1, 0, 0)]));
        registry.add_pack(pack_meta(
            "pack-a",
            vec![release_with_deps(1, 0, 0, &[("pack-b", "^1.0.0")])],
        ));

        let resolver = Resolver::new(&registry);
        let profile = Profile {
            name: "test".into(),
            packs: vec![],
        };

        let plan = resolver.plan_install("pack-a", None, &profile).unwrap();

        let names: Vec<&str> = plan.to_install.iter().map(|(n, _)| n.as_str()).collect();
        assert!(names.contains(&"pack-a"), "pack-a should be in to_install");
        assert!(
            names.contains(&"pack-b"),
            "transitive dep pack-b should be in to_install"
        );
        assert_eq!(plan.to_install.len(), 2);
    }

    /// If pack B is already installed at the correct version when pack A is
    /// installed, B should appear in `already_satisfied`, not `to_install`.
    #[test]
    fn already_satisfied_transitive() {
        let mut registry = MockRegistry::new();
        registry.add_pack(pack_meta("pack-b", vec![release(1, 0, 0)]));
        registry.add_pack(pack_meta(
            "pack-a",
            vec![release_with_deps(1, 0, 0, &[("pack-b", "^1.0.0")])],
        ));

        let resolver = Resolver::new(&registry);
        let profile = Profile {
            name: "test".into(),
            packs: vec![InstalledPack {
                name: "pack-b".into(),
                version: semver::Version::new(1, 0, 0),
                source: PackSource::Registry {
                    registry_url: "https://example.com".into(),
                },
            }],
        };

        let plan = resolver.plan_install("pack-a", None, &profile).unwrap();

        let install_names: Vec<&str> = plan.to_install.iter().map(|(n, _)| n.as_str()).collect();
        assert!(install_names.contains(&"pack-a"), "pack-a should be queued");
        assert!(
            !install_names.contains(&"pack-b"),
            "pack-b is satisfied; should not be in to_install"
        );
        assert!(
            plan.already_satisfied.contains(&"pack-b".to_string()),
            "pack-b should be in already_satisfied"
        );
    }

    /// A circular dependency (A → B → A) must return a CircularDependency error
    /// rather than looping forever.
    #[test]
    fn circular_dependency_returns_error() {
        let mut registry = MockRegistry::new();
        registry.add_pack(pack_meta(
            "pack-b",
            vec![release_with_deps(1, 0, 0, &[("pack-a", "^1.0.0")])],
        ));
        registry.add_pack(pack_meta(
            "pack-a",
            vec![release_with_deps(1, 0, 0, &[("pack-b", "^1.0.0")])],
        ));

        let resolver = Resolver::new(&registry);
        let profile = Profile {
            name: "test".into(),
            packs: vec![],
        };

        let result = resolver.plan_install("pack-a", None, &profile);
        assert!(
            matches!(result, Err(WeaveError::CircularDependency { .. })),
            "expected CircularDependency error, got: {result:?}"
        );
    }

    /// A deep chain A → B → C should install all three packs.
    #[test]
    fn deep_transitive_chain() {
        let mut registry = MockRegistry::new();
        registry.add_pack(pack_meta("pack-c", vec![release(1, 0, 0)]));
        registry.add_pack(pack_meta(
            "pack-b",
            vec![release_with_deps(1, 0, 0, &[("pack-c", "^1.0.0")])],
        ));
        registry.add_pack(pack_meta(
            "pack-a",
            vec![release_with_deps(1, 0, 0, &[("pack-b", "^1.0.0")])],
        ));

        let resolver = Resolver::new(&registry);
        let profile = Profile {
            name: "test".into(),
            packs: vec![],
        };

        let plan = resolver.plan_install("pack-a", None, &profile).unwrap();

        let names: Vec<&str> = plan.to_install.iter().map(|(n, _)| n.as_str()).collect();
        assert!(names.contains(&"pack-a"), "pack-a should be installed");
        assert!(names.contains(&"pack-b"), "pack-b should be installed");
        assert!(names.contains(&"pack-c"), "pack-c should be installed");
        assert_eq!(plan.to_install.len(), 3);
    }
}
