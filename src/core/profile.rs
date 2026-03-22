use std::path::PathBuf;

use serde::{Deserialize, Serialize};

use crate::core::pack::PackSource;
use crate::error::{Result, WeaveError};
use crate::util;

/// A named collection of installed packs.
/// Stored at `~/.packweave/profiles/<name>.toml`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Profile {
    pub name: String,
    #[serde(default)]
    pub packs: Vec<InstalledPack>,
}

/// A pack recorded as installed in a profile.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InstalledPack {
    pub name: String,
    pub version: semver::Version,
    pub source: PackSource,
}

impl Profile {
    /// Directory where all profiles are stored.
    fn profiles_dir() -> Result<PathBuf> {
        Ok(util::packweave_dir()?.join("profiles"))
    }

    /// Path to this profile's file.
    fn path(name: &str) -> Result<PathBuf> {
        Ok(Self::profiles_dir()?.join(format!("{name}.toml")))
    }

    /// Load a profile by name, creating it if it doesn't exist.
    pub fn load(name: &str) -> Result<Self> {
        let path = Self::path(name)?;
        if !path.exists() {
            return Ok(Self {
                name: name.to_string(),
                packs: Vec::new(),
            });
        }
        let content = util::read_file(&path)?;
        toml::from_str(&content).map_err(|e| WeaveError::Toml {
            path,
            source: Box::new(e),
        })
    }

    /// Save this profile to disk.
    pub fn save(&self) -> Result<()> {
        let path = Self::path(&self.name)?;
        // Profile only contains String/Vec fields — TOML serialization cannot fail.
        let content = toml::to_string_pretty(self).expect("Profile serialization cannot fail");
        util::write_file(&path, &content)
    }

    /// Check whether a profile file exists on disk.
    pub fn exists(name: &str) -> Result<bool> {
        Ok(Self::path(name)?.exists())
    }

    /// List all saved profile names (profiles that have been saved to disk).
    pub fn list_all() -> Result<Vec<String>> {
        let dir = Self::profiles_dir()?;
        if !dir.exists() {
            return Ok(Vec::new());
        }
        let mut names = Vec::new();
        let entries =
            std::fs::read_dir(&dir).map_err(|e| WeaveError::io("listing profiles directory", e))?;
        for entry in entries {
            let entry = entry.map_err(|e| WeaveError::io("reading profiles directory entry", e))?;
            let path = entry.path();
            if path.extension().and_then(|e| e.to_str()) == Some("toml") {
                if let Some(stem) = path.file_stem().and_then(|s| s.to_str()) {
                    names.push(stem.to_string());
                }
            }
        }
        names.sort();
        Ok(names)
    }

    /// Delete a profile from disk. Returns an error if the file does not exist.
    pub fn delete(name: &str) -> Result<()> {
        let path = Self::path(name)?;
        if !path.exists() {
            return Err(WeaveError::ProfileNotFound {
                name: name.to_string(),
            });
        }
        std::fs::remove_file(&path)
            .map_err(|e| WeaveError::io(format!("deleting profile '{name}'"), e))
    }

    /// Check if a pack is installed in this profile.
    pub fn has_pack(&self, name: &str) -> bool {
        self.packs.iter().any(|p| p.name == name)
    }

    /// Get an installed pack by name.
    pub fn get_pack(&self, name: &str) -> Option<&InstalledPack> {
        self.packs.iter().find(|p| p.name == name)
    }

    /// Add or update a pack in the profile.
    pub fn add_pack(&mut self, pack: InstalledPack) {
        self.packs.retain(|p| p.name != pack.name);
        self.packs.push(pack);
    }

    /// Remove a pack from the profile. Returns true if it was present.
    pub fn remove_pack(&mut self, name: &str) -> bool {
        let before = self.packs.len();
        self.packs.retain(|p| p.name != name);
        self.packs.len() < before
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_pack(name: &str) -> InstalledPack {
        InstalledPack {
            name: name.to_string(),
            version: semver::Version::new(1, 0, 0),
            source: PackSource::Registry {
                registry_url: "https://example.com".into(),
            },
        }
    }

    #[test]
    fn add_and_remove_packs() {
        let mut profile = Profile {
            name: "test".into(),
            packs: Vec::new(),
        };

        profile.add_pack(test_pack("webdev"));
        assert!(profile.has_pack("webdev"));
        assert_eq!(profile.packs.len(), 1);

        // Adding same name replaces
        profile.add_pack(test_pack("webdev"));
        assert_eq!(profile.packs.len(), 1);

        assert!(profile.remove_pack("webdev"));
        assert!(!profile.has_pack("webdev"));
        assert!(!profile.remove_pack("webdev"));
    }

    #[test]
    fn list_all_and_delete_with_temp_dir() {
        let tmp = tempfile::TempDir::new().unwrap();
        // Point WEAVE_TEST_STORE_DIR so profile I/O goes to our temp dir
        std::env::set_var("WEAVE_TEST_STORE_DIR", tmp.path());

        // Create two profiles
        let p1 = Profile {
            name: "alpha".into(),
            packs: Vec::new(),
        };
        p1.save().unwrap();

        let p2 = Profile {
            name: "beta".into(),
            packs: vec![test_pack("webdev")],
        };
        p2.save().unwrap();

        // list_all should return both
        let all = Profile::list_all().unwrap();
        assert!(all.contains(&"alpha".to_string()));
        assert!(all.contains(&"beta".to_string()));

        // exists should return true
        assert!(Profile::exists("alpha").unwrap());
        assert!(Profile::exists("beta").unwrap());
        assert!(!Profile::exists("nonexistent").unwrap());

        // delete alpha
        Profile::delete("alpha").unwrap();
        assert!(!Profile::exists("alpha").unwrap());

        // delete nonexistent should fail
        let result = Profile::delete("nonexistent");
        assert!(result.is_err());

        std::env::remove_var("WEAVE_TEST_STORE_DIR");
    }
}
