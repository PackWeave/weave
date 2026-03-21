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

        self.resolve_pack(
            pack_name,
            version_req,
            profile,
            &mut to_install,
            &mut already_satisfied,
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
    ) -> Result<()> {
        // Skip if already queued for installation
        if to_install.iter().any(|(n, _)| n == pack_name) {
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
                return Ok(());
            }
        }

        to_install.push((pack_name.to_string(), version));

        // For v1, we don't recursively resolve dependencies from the registry.
        // Pack dependencies would require fetching and parsing each pack's manifest.
        // This will be enhanced in later milestones.

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::pack::PackSource;
    use crate::core::profile::InstalledPack;
    use crate::core::registry::{MockRegistry, PackMetadata, PackRelease};

    fn setup_registry() -> MockRegistry {
        let mut registry = MockRegistry::new();
        registry.add_pack(PackMetadata {
            name: "webdev".into(),
            description: "Web tools".into(),
            authors: vec![],
            license: None,
            repository: None,
            versions: vec![
                PackRelease {
                    version: semver::Version::new(1, 0, 0),
                    url: "https://example.com/webdev-1.0.0.tar.gz".into(),
                    sha256: "abc".into(),
                    size_bytes: None,
                },
                PackRelease {
                    version: semver::Version::new(1, 1, 0),
                    url: "https://example.com/webdev-1.1.0.tar.gz".into(),
                    sha256: "def".into(),
                    size_bytes: None,
                },
            ],
        });
        registry
    }

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
}
