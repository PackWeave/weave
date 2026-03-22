use std::io::Read;
use std::path::PathBuf;

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

    /// Write a pack release's inline file content into the store.
    ///
    /// If the pack version is already cached (`pack.toml` exists), returns
    /// immediately without touching the filesystem.
    ///
    /// Uses an atomic staging pattern: files are written to a `.tmp` directory
    /// first, then renamed to the final destination so a failure never leaves
    /// a partial cache entry.
    pub fn fetch(name: &str, release: &PackRelease) -> Result<PathBuf> {
        let dest = Self::pack_dir(name, &release.version)?;

        // If already cached, return early.
        if dest.join("pack.toml").exists() {
            return Ok(dest);
        }

        // Stage into a temporary directory to ensure atomicity.
        //
        // Note: `dest.with_extension("tmp")` is WRONG for semver paths like `1.1.0` —
        // it treats "0" as the extension and produces `1.1.tmp` instead of `1.1.0.tmp`,
        // causing collisions between different patch versions. Append to the full name.
        let tmp_dest = dest
            .parent()
            .expect("pack_dir is always <store>/<name>/<version>, so parent always exists")
            .join({
                let mut n = dest
                    .file_name()
                    .expect("pack_dir always has a version component as its final segment")
                    .to_os_string();
                n.push(".tmp");
                n
            });
        if tmp_dest.exists() {
            std::fs::remove_dir_all(&tmp_dest)
                .map_err(|e| WeaveError::io(format!("cleaning up tmp dir for '{name}'"), e))?;
        }
        util::ensure_dir(&tmp_dest)?;

        if let Err(e) = Self::write_files(name, &release.files, &tmp_dest) {
            let _ = std::fs::remove_dir_all(&tmp_dest);
            return Err(e);
        }

        // Atomically promote the staging directory to the final location.
        //
        // Check again whether a valid cache entry appeared while we were writing
        // (e.g. concurrent install by another process). If so, discard our staging
        // copy and reuse the existing entry.
        if dest.join("pack.toml").exists() {
            let _ = std::fs::remove_dir_all(&tmp_dest);
            return Ok(dest);
        }
        // If `dest` exists but has no `pack.toml` it is a leftover from a previous
        // failed run or a manual directory. Remove it so the rename can succeed.
        if dest.exists() {
            std::fs::remove_dir_all(&dest)
                .map_err(|e| WeaveError::io(format!("removing stale dir for '{name}'"), e))?;
        }
        std::fs::rename(&tmp_dest, &dest)
            .map_err(|e| WeaveError::io(format!("finalizing pack '{name}'"), e))?;

        Ok(dest)
    }

    /// Write the inline file map from a `PackRelease` into `dest`.
    ///
    /// Each key is a relative path; the value is the file content. Applies the
    /// same path-safety rules that tarball extraction used: rejects absolute
    /// paths, `..` components, and Windows drive prefixes. These checks are
    /// necessary because `files` keys come from untrusted registry content.
    fn write_files(
        name: &str,
        files: &std::collections::HashMap<String, String>,
        dest: &std::path::Path,
    ) -> Result<()> {
        let dest_canonical = dest
            .canonicalize()
            .map_err(|e| WeaveError::io(format!("canonicalizing dest dir for '{name}'"), e))?;

        for (rel_path, content) in files {
            let entry_path = std::path::Path::new(rel_path);

            // Reject absolute paths and leading `/` or `\` (cross-platform).
            let path_str = entry_path.to_string_lossy();
            if entry_path.is_absolute() || path_str.starts_with('/') || path_str.starts_with('\\') {
                return Err(WeaveError::io(
                    format!("writing pack '{name}'"),
                    std::io::Error::new(
                        std::io::ErrorKind::InvalidData,
                        format!("file path has absolute component: {rel_path}"),
                    ),
                ));
            }

            // Reject `..` components and Windows drive/prefix components.
            if entry_path.components().any(|c| {
                matches!(
                    c,
                    std::path::Component::ParentDir | std::path::Component::Prefix(_)
                )
            }) {
                return Err(WeaveError::io(
                    format!("writing pack '{name}'"),
                    std::io::Error::new(
                        std::io::ErrorKind::InvalidData,
                        format!("file path '{rel_path}' would escape the pack directory"),
                    ),
                ));
            }

            let full_path = dest_canonical.join(entry_path);

            // Create parent directories for nested paths (e.g. prompts/system.md).
            if let Some(parent) = full_path.parent() {
                std::fs::create_dir_all(parent).map_err(|e| {
                    WeaveError::io(format!("creating dirs for '{rel_path}' in '{name}'"), e)
                })?;
            }

            std::fs::write(&full_path, content.as_bytes())
                .map_err(|e| WeaveError::io(format!("writing '{rel_path}' for '{name}'"), e))?;
        }

        Ok(())
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
                if let Err(e) = std::fs::remove_dir(&name_dir) {
                    log::warn!(
                        "could not remove empty pack directory {}: {e}",
                        name_dir.display()
                    );
                }
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

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;
    use tempfile::TempDir;

    // ── write_files ───────────────────────────────────────────────────────────

    #[test]
    fn write_files_creates_expected_files() {
        let dir = TempDir::new().unwrap();
        let files = HashMap::from([
            (
                "pack.toml".to_string(),
                "[pack]\nname = \"test\"".to_string(),
            ),
            (
                "prompts/system.md".to_string(),
                "You are helpful.".to_string(),
            ),
            ("commands/review.md".to_string(), "# Review".to_string()),
        ]);
        Store::write_files("test", &files, dir.path()).unwrap();
        assert_eq!(
            std::fs::read_to_string(dir.path().join("pack.toml")).unwrap(),
            "[pack]\nname = \"test\""
        );
        assert_eq!(
            std::fs::read_to_string(dir.path().join("prompts/system.md")).unwrap(),
            "You are helpful."
        );
        assert_eq!(
            std::fs::read_to_string(dir.path().join("commands/review.md")).unwrap(),
            "# Review"
        );
    }

    #[test]
    fn write_files_rejects_path_traversal() {
        let dir = TempDir::new().unwrap();
        let files = HashMap::from([("../escape".to_string(), "evil".to_string())]);
        let result = Store::write_files("evil-pack", &files, dir.path());
        assert!(result.is_err(), "path traversal should be rejected");
        assert!(
            !dir.path().parent().unwrap().join("escape").exists(),
            "traversal file must not be created"
        );
    }

    #[test]
    fn write_files_rejects_absolute_path() {
        let dir = TempDir::new().unwrap();
        let files = HashMap::from([("/etc/evil".to_string(), "evil".to_string())]);
        let result = Store::write_files("evil-pack", &files, dir.path());
        assert!(result.is_err(), "absolute path should be rejected");
    }

    #[test]
    fn write_files_empty_map_succeeds() {
        let dir = TempDir::new().unwrap();
        let files = HashMap::new();
        assert!(Store::write_files("empty", &files, dir.path()).is_ok());
    }

    #[test]
    fn write_files_rejects_backslash_absolute() {
        let dir = TempDir::new().unwrap();
        let files = HashMap::from(["\\windows\\evil".to_string()].map(|p| (p, "x".to_string())));
        let result = Store::write_files("evil-pack", &files, dir.path());
        assert!(
            result.is_err(),
            "backslash absolute path should be rejected"
        );
    }
}
