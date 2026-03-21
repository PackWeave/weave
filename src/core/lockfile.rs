use std::collections::BTreeMap;
use std::path::PathBuf;

use serde::{Deserialize, Serialize};

use crate::error::{Result, WeaveError};
use crate::util;

/// Lock file pinning exact resolved versions for a profile.
/// Stored at `~/.packweave/locks/<profile>.lock`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LockFile {
    #[serde(default)]
    pub packs: BTreeMap<String, LockedPack>,
}

/// A single locked pack entry with its exact version and checksum.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LockedPack {
    pub version: semver::Version,
    #[serde(default)]
    pub sha256: Option<String>,
    #[serde(default)]
    pub source: Option<String>,
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
                packs: BTreeMap::new(),
            });
        }
        let content = util::read_file(&path)?;
        toml::from_str(&content).map_err(|e| WeaveError::Toml { path, source: e })
    }

    /// Save this lock file to disk.
    pub fn save(&self, profile_name: &str) -> Result<()> {
        let path = Self::path(profile_name)?;
        let content = toml::to_string_pretty(self).expect("LockFile serialization cannot fail");
        util::write_file(&path, &content)
    }

    /// Record a pack's resolved version.
    pub fn lock_pack(&mut self, name: &str, version: semver::Version, sha256: Option<String>) {
        self.packs.insert(
            name.to_string(),
            LockedPack {
                version,
                sha256,
                source: None,
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
            packs: BTreeMap::new(),
        };

        lock.lock_pack("webdev", semver::Version::new(1, 2, 3), Some("abc".into()));
        assert_eq!(lock.packs["webdev"].version, semver::Version::new(1, 2, 3));
        assert_eq!(lock.packs["webdev"].sha256.as_deref(), Some("abc"));

        lock.unlock_pack("webdev");
        assert!(lock.packs.is_empty());
    }

    #[test]
    fn roundtrip_toml() {
        let mut lock = LockFile {
            packs: BTreeMap::new(),
        };
        lock.lock_pack("test", semver::Version::new(0, 1, 0), None);

        let toml_str = toml::to_string_pretty(&lock).unwrap();
        let parsed: LockFile = toml::from_str(&toml_str).unwrap();
        assert_eq!(parsed.packs["test"].version, semver::Version::new(0, 1, 0));
    }
}
