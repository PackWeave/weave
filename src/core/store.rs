use std::io::Read;
use std::path::PathBuf;

use sha2::{Digest, Sha256};

use crate::core::pack::Pack;
use crate::core::registry::PackRelease;
use crate::error::{Result, WeaveError};
use crate::util;

/// Manages the local pack cache at `~/.packweave/packs/`.
pub struct Store;

impl Store {
    /// Root directory of the store.
    pub fn root() -> Result<PathBuf> {
        Ok(util::packweave_dir()?.join("packs"))
    }

    /// Path where a specific pack version is stored.
    pub fn pack_dir(name: &str, version: &semver::Version) -> Result<PathBuf> {
        Ok(Self::root()?.join(name).join(version.to_string()))
    }

    /// Check if a pack version is already cached locally.
    pub fn is_cached(name: &str, version: &semver::Version) -> Result<bool> {
        let dir = Self::pack_dir(name, version)?;
        Ok(dir.join("pack.toml").exists())
    }

    /// Download, verify, and extract a pack release into the store.
    pub fn fetch(name: &str, release: &PackRelease) -> Result<PathBuf> {
        let dest = Self::pack_dir(name, &release.version)?;

        // If already cached, return early
        if dest.join("pack.toml").exists() {
            return Ok(dest);
        }

        // Download the archive
        let response =
            reqwest::blocking::get(&release.url).map_err(|e| WeaveError::DownloadFailed {
                name: name.to_string(),
                reason: e.to_string(),
            })?;

        if !response.status().is_success() {
            return Err(WeaveError::DownloadFailed {
                name: name.to_string(),
                reason: format!("HTTP {}", response.status()),
            });
        }

        let bytes = response.bytes().map_err(|e| WeaveError::DownloadFailed {
            name: name.to_string(),
            reason: e.to_string(),
        })?;

        // Verify SHA256
        let mut hasher = Sha256::new();
        hasher.update(&bytes);
        let actual_hash = format!("{:x}", hasher.finalize());

        if actual_hash != release.sha256 {
            return Err(WeaveError::ChecksumMismatch {
                name: name.to_string(),
                expected: release.sha256.clone(),
                actual: actual_hash,
            });
        }

        // Extract tar.gz
        util::ensure_dir(&dest)?;
        let decoder = flate2::read::GzDecoder::new(&bytes[..]);
        let mut archive = tar::Archive::new(decoder);
        archive
            .unpack(&dest)
            .map_err(|e| WeaveError::io(format!("extracting pack '{name}'"), e))?;

        Ok(dest)
    }

    /// Load a pack manifest from the store.
    pub fn load_pack(name: &str, version: &semver::Version) -> Result<Pack> {
        let dir = Self::pack_dir(name, version)?;
        Pack::load(&dir)
    }

    /// List all cached packs as (name, version) pairs.
    pub fn list_cached() -> Result<Vec<(String, semver::Version)>> {
        let root = Self::root()?;
        let mut result = Vec::new();

        if !root.exists() {
            return Ok(result);
        }

        let entries = std::fs::read_dir(&root).map_err(|e| WeaveError::io("listing store", e))?;

        for entry in entries {
            let entry = entry.map_err(|e| WeaveError::io("reading store entry", e))?;
            let name = entry.file_name().to_string_lossy().to_string();

            if !entry.path().is_dir() {
                continue;
            }

            let versions = std::fs::read_dir(entry.path())
                .map_err(|e| WeaveError::io("listing pack versions", e))?;

            for ver_entry in versions {
                let ver_entry =
                    ver_entry.map_err(|e| WeaveError::io("reading version entry", e))?;
                let ver_str = ver_entry.file_name().to_string_lossy().to_string();
                if let Ok(version) = semver::Version::parse(&ver_str) {
                    if ver_entry.path().join("pack.toml").exists() {
                        result.push((name.clone(), version));
                    }
                }
            }
        }

        result.sort_by(|a, b| a.0.cmp(&b.0).then(a.1.cmp(&b.1)));
        Ok(result)
    }

    /// Remove a specific pack version from the store.
    pub fn evict(name: &str, version: &semver::Version) -> Result<()> {
        let dir = Self::pack_dir(name, version)?;
        if dir.exists() {
            std::fs::remove_dir_all(&dir)
                .map_err(|e| WeaveError::io(format!("evicting {name}@{version}"), e))?;
        }

        // Clean up the name directory if empty
        let name_dir = Self::root()?.join(name);
        if name_dir.exists() {
            let is_empty = std::fs::read_dir(&name_dir)
                .map(|mut d| d.next().is_none())
                .unwrap_or(false);
            if is_empty {
                let _ = std::fs::remove_dir(&name_dir);
            }
        }

        Ok(())
    }

    /// Read a file from a cached pack, returning None if it doesn't exist.
    pub fn read_pack_file(
        name: &str,
        version: &semver::Version,
        relative_path: &str,
    ) -> Result<Option<String>> {
        let path = Self::pack_dir(name, version)?.join(relative_path);
        if !path.exists() {
            return Ok(None);
        }
        let mut content = String::new();
        std::fs::File::open(&path)
            .and_then(|mut f| f.read_to_string(&mut content))
            .map_err(|e| WeaveError::io(format!("reading {}", path.display()), e))?;
        Ok(Some(content))
    }
}
