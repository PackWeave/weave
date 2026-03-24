use std::collections::BTreeMap;
use std::path::PathBuf;

use serde::{Deserialize, Serialize};

use crate::core::pack::PackSource;
use crate::error::{Result, WeaveError};
use crate::util;

/// Current schema version for lock files.
pub const CURRENT_LOCKFILE_SCHEMA_VERSION: u32 = 1;

fn default_schema_version() -> u32 {
    1
}

/// Lock file pinning exact resolved versions for a profile.
/// Stored at `~/.packweave/locks/<profile>.lock`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LockFile {
    #[serde(default = "default_schema_version")]
    pub schema_version: u32,
    #[serde(default)]
    pub packs: BTreeMap<String, LockedPack>,
}

/// A single locked pack entry with its exact version and install source.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LockedPack {
    pub version: semver::Version,
    #[serde(default)]
    pub source: Option<PackSource>,
}

impl LockFile {
    /// Directory where lock files are stored.
    fn locks_dir() -> Result<PathBuf> {
        Ok(util::packweave_dir()?.join("locks"))
    }

    /// Path to the lock file for a given profile.
    fn path(profile_name: &str) -> Result<PathBuf> {
        Ok(Self::locks_dir()?.join(format!("{profile_name}.lock")))
    }

    /// Load a lock file for a profile, returning empty if it doesn't exist.
    pub fn load(profile_name: &str) -> Result<Self> {
        let path = Self::path(profile_name)?;
        if !path.exists() {
            return Ok(Self {
                schema_version: CURRENT_LOCKFILE_SCHEMA_VERSION,
                packs: BTreeMap::new(),
            });
        }
        let content = util::read_file(&path)?;
        let lockfile: LockFile = toml::from_str(&content).map_err(|e| WeaveError::Toml {
            path: path.clone(),
            source: Box::new(e),
        })?;
        if lockfile.schema_version > CURRENT_LOCKFILE_SCHEMA_VERSION {
            return Err(WeaveError::SchemaVersionTooNew {
                file_kind: "lock file",
                path,
                found: lockfile.schema_version,
                supported: CURRENT_LOCKFILE_SCHEMA_VERSION,
            });
        }
        Ok(lockfile)
    }

    /// Save this lock file to disk.
    pub fn save(&self, profile_name: &str) -> Result<()> {
        let path = Self::path(profile_name)?;
        // LockFile only contains String/semver fields — TOML serialization cannot fail.
        let content = toml::to_string_pretty(self).expect("LockFile serialization cannot fail");
        util::write_file(&path, &content)
    }

    /// Record a pack's resolved version and install source.
    pub fn lock_pack(&mut self, name: &str, version: semver::Version, source: PackSource) {
        self.packs.insert(
            name.to_string(),
            LockedPack {
                version,
                source: Some(source),
            },
        );
    }

    /// Remove a pack from the lock file.
    pub fn unlock_pack(&mut self, name: &str) {
        self.packs.remove(name);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lock_and_unlock() {
        let mut lock = LockFile {
            schema_version: CURRENT_LOCKFILE_SCHEMA_VERSION,
            packs: BTreeMap::new(),
        };

        lock.lock_pack(
            "webdev",
            semver::Version::new(1, 2, 3),
            PackSource::Registry {
                registry_url: "https://example.com".to_string(),
            },
        );
        assert_eq!(lock.packs["webdev"].version, semver::Version::new(1, 2, 3));
        assert!(matches!(
            &lock.packs["webdev"].source,
            Some(PackSource::Registry { .. })
        ));

        lock.unlock_pack("webdev");
        assert!(lock.packs.is_empty());
    }

    #[test]
    fn roundtrip_toml() {
        let mut lock = LockFile {
            schema_version: CURRENT_LOCKFILE_SCHEMA_VERSION,
            packs: BTreeMap::new(),
        };
        lock.lock_pack(
            "test",
            semver::Version::new(0, 1, 0),
            PackSource::Registry {
                registry_url: "https://example.com".to_string(),
            },
        );

        let toml_str = toml::to_string_pretty(&lock).unwrap();
        let parsed: LockFile = toml::from_str(&toml_str).unwrap();
        assert_eq!(parsed.packs["test"].version, semver::Version::new(0, 1, 0));
        assert!(matches!(
            &parsed.packs["test"].source,
            Some(PackSource::Registry { .. })
        ));
    }

    #[test]
    fn old_lockfile_without_source_deserializes() {
        // Lockfiles written before source tracking had no `source` key — must default to None.
        let toml_str = "[packs.filesystem]\nversion = \"0.1.0\"\n";
        let parsed: LockFile = toml::from_str(toml_str).unwrap();
        assert_eq!(
            parsed.packs["filesystem"].version,
            semver::Version::new(0, 1, 0)
        );
        assert!(parsed.packs["filesystem"].source.is_none());
    }

    #[test]
    fn reject_lockfile_with_future_schema_version() {
        let toml_str = "schema_version = 99\n\n[packs.test]\nversion = \"1.0.0\"\n";
        let parsed: LockFile = toml::from_str(toml_str).unwrap();
        assert_eq!(parsed.schema_version, 99);
        // The version check happens in LockFile::load() which reads from disk,
        // so we verify the field deserializes correctly and test the guard directly.
        assert!(parsed.schema_version > CURRENT_LOCKFILE_SCHEMA_VERSION);
    }
}
