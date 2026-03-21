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

        // If already cached, return early.
        if dest.join("pack.toml").exists() {
            return Ok(dest);
        }

        // Download the archive.
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

        Self::verify_checksum(name, &bytes, &release.sha256)?;

        // Extract into a temporary staging directory first, then rename it to the
        // final destination so a failed extraction never leaves a partial cache entry.
        //
        // Note: `dest.with_extension("tmp")` is WRONG for semver paths like `1.1.0` —
        // it treats "0" as the extension and produces `1.1.tmp` instead of `1.1.0.tmp`,
        // causing collisions between different patch versions. Append to the full name.
        let tmp_dest = dest
            .parent()
            .expect("pack_dir is always <store>/<name>/<version>, so parent always exists")
            .join({
                let mut name = dest
                    .file_name()
                    .expect("pack_dir always has a version component as its final segment")
                    .to_os_string();
                name.push(".tmp");
                name
            });
        if tmp_dest.exists() {
            std::fs::remove_dir_all(&tmp_dest)
                .map_err(|e| WeaveError::io(format!("cleaning up tmp dir for '{name}'"), e))?;
        }
        util::ensure_dir(&tmp_dest)?;

        if let Err(e) = Self::extract_archive(name, &bytes, &tmp_dest) {
            let _ = std::fs::remove_dir_all(&tmp_dest);
            return Err(e);
        }

        // Atomically promote the staging directory to the final location.
        std::fs::rename(&tmp_dest, &dest)
            .map_err(|e| WeaveError::io(format!("finalizing pack '{name}'"), e))?;

        Ok(dest)
    }

    /// Verify the SHA256 checksum of raw archive bytes.
    /// Both sides are normalised to lowercase so mixed-case registry hashes match.
    fn verify_checksum(name: &str, bytes: &[u8], expected_sha256: &str) -> Result<()> {
        let mut hasher = Sha256::new();
        hasher.update(bytes);
        let actual = format!("{:x}", hasher.finalize());
        if actual != expected_sha256.to_lowercase() {
            return Err(WeaveError::ChecksumMismatch {
                name: name.to_string(),
                expected: expected_sha256.to_string(),
                actual,
            });
        }
        Ok(())
    }

    /// Extract a tar.gz archive into `dest`, rejecting any entry whose resolved
    /// path would escape the destination directory (tar-slip protection).
    fn extract_archive(name: &str, bytes: &[u8], dest: &std::path::Path) -> Result<()> {
        let decoder = flate2::read::GzDecoder::new(bytes);
        let mut archive = tar::Archive::new(decoder);
        let dest_canonical = dest
            .canonicalize()
            .map_err(|e| WeaveError::io(format!("canonicalizing dest dir for '{name}'"), e))?;

        for entry in archive
            .entries()
            .map_err(|e| WeaveError::io(format!("reading archive entries for '{name}'"), e))?
        {
            let mut entry = entry
                .map_err(|e| WeaveError::io(format!("reading archive entry for '{name}'"), e))?;
            let entry_path = entry
                .path()
                .map_err(|e| WeaveError::io(format!("reading entry path in '{name}'"), e))?
                .into_owned();

            // Reject absolute paths and any `..` component (tar-slip protection).
            // Note: starts_with() alone is insufficient for traversal detection because
            // dest.join("../evil").starts_with(dest) returns true; we check components directly.
            //
            // is_absolute() is not enough on Windows — Path::is_absolute() requires a drive
            // letter (C:\) and won't catch POSIX-style "/etc/evil" paths that appear in tar
            // archives produced on Linux/macOS. Check the raw string as well.
            let path_str = entry_path.to_string_lossy();
            if entry_path.is_absolute() || path_str.starts_with('/') || path_str.starts_with('\\') {
                return Err(WeaveError::io(
                    format!("extracting pack '{name}'"),
                    std::io::Error::new(
                        std::io::ErrorKind::InvalidData,
                        format!("archive entry has absolute path: {}", entry_path.display()),
                    ),
                ));
            }
            // Reject `..` components (tar-slip) and Windows drive/prefix components.
            // On Windows, PathBuf::join discards the base path when the joined component
            // contains a Prefix (e.g. "C:evil"), causing silent path replacement — a
            // tar-slip vector that bypasses absolute-path and `..` checks.
            if entry_path.components().any(|c| {
                matches!(
                    c,
                    std::path::Component::ParentDir | std::path::Component::Prefix(_)
                )
            }) {
                return Err(WeaveError::io(
                    format!("extracting pack '{name}'"),
                    std::io::Error::new(
                        std::io::ErrorKind::InvalidData,
                        format!(
                            "archive entry '{}' would escape the pack directory",
                            entry_path.display()
                        ),
                    ),
                ));
            }

            // Reject symlinks and hardlinks.  A symlink entry pointing outside `dest`
            // followed by a regular-file entry written *through* it would bypass all
            // the path-component checks above (no `..`, no absolute path — yet the
            // bytes land outside the pack directory).
            let entry_type = entry.header().entry_type();
            if entry_type.is_symlink() || entry_type.is_hard_link() {
                return Err(WeaveError::io(
                    format!("extracting pack '{name}'"),
                    std::io::Error::new(
                        std::io::ErrorKind::InvalidData,
                        format!(
                            "archive entry '{}' is a symlink or hardlink — not permitted in packs",
                            entry_path.display()
                        ),
                    ),
                ));
            }

            let full_path = dest_canonical.join(&entry_path);

            // Create parent directories so nested paths (e.g. prompts/claude.md) unpack correctly.
            if let Some(parent) = full_path.parent() {
                std::fs::create_dir_all(parent).map_err(|e| {
                    WeaveError::io(
                        format!("creating dirs for '{}' in '{name}'", entry_path.display()),
                        e,
                    )
                })?;
            }

            entry.unpack(&full_path).map_err(|e| {
                WeaveError::io(
                    format!("extracting '{}' from '{name}'", entry_path.display()),
                    e,
                )
            })?;
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
    use std::io::Write;
    use tempfile::TempDir;

    /// Build a minimal tar.gz in memory containing the given files.
    fn make_tar_gz(files: &[(&str, &[u8])]) -> Vec<u8> {
        let mut tar_bytes = Vec::new();
        {
            let mut builder = tar::Builder::new(&mut tar_bytes);
            for (path, content) in files {
                let mut header = tar::Header::new_gnu();
                header.set_size(content.len() as u64);
                header.set_mode(0o644);
                header.set_cksum();
                builder.append_data(&mut header, path, *content).unwrap();
            }
            builder.finish().unwrap();
        }
        let mut gz_bytes = Vec::new();
        let mut encoder =
            flate2::write::GzEncoder::new(&mut gz_bytes, flate2::Compression::default());
        encoder.write_all(&tar_bytes).unwrap();
        encoder.finish().unwrap();
        gz_bytes
    }

    /// Compute the lowercase hex SHA256 of `bytes`.
    fn sha256_hex(bytes: &[u8]) -> String {
        let mut hasher = Sha256::new();
        hasher.update(bytes);
        format!("{:x}", hasher.finalize())
    }

    // ── verify_checksum ───────────────────────────────────────────────────────

    #[test]
    fn checksum_correct_passes() {
        let bytes = b"hello world";
        let hash = sha256_hex(bytes);
        assert!(Store::verify_checksum("pack", bytes, &hash).is_ok());
    }

    #[test]
    fn checksum_uppercase_hash_passes() {
        let bytes = b"hello world";
        let hash = sha256_hex(bytes).to_uppercase();
        assert!(
            Store::verify_checksum("pack", bytes, &hash).is_ok(),
            "uppercase SHA256 from registry should match"
        );
    }

    #[test]
    fn checksum_mismatch_returns_error() {
        let bytes = b"hello world";
        let result = Store::verify_checksum("pack", bytes, "deadbeef");
        assert!(result.is_err());
        let err = result.unwrap_err();
        assert!(
            err.to_string().contains("checksum"),
            "error should mention checksum: {err}"
        );
    }

    // ── extract_archive ───────────────────────────────────────────────────────

    #[test]
    fn extract_valid_archive_succeeds() {
        let dir = TempDir::new().unwrap();
        let bytes = make_tar_gz(&[("pack.toml", b"name = \"test\"")]);
        Store::extract_archive("test", &bytes, dir.path()).unwrap();
        assert!(
            dir.path().join("pack.toml").exists(),
            "pack.toml should be extracted"
        );
        let content = std::fs::read_to_string(dir.path().join("pack.toml")).unwrap();
        assert_eq!(content, "name = \"test\"");
    }

    /// Build a tar.gz with a single entry using a raw header, bypassing the
    /// tar crate's path validation. This lets us craft malicious archives that
    /// real attackers could produce to test our extraction defences.
    fn make_raw_tar_gz(path: &str, content: &[u8]) -> Vec<u8> {
        make_raw_tar_gz_with_type(path, content, b'0')
    }

    /// Build a tar.gz with a raw header and an explicit typeflag byte.
    /// typeflag values: b'0' = regular file, b'1' = hardlink, b'2' = symlink.
    fn make_raw_tar_gz_with_type(path: &str, content: &[u8], typeflag: u8) -> Vec<u8> {
        let mut header = [0u8; 512];

        // Name field (bytes 0–99)
        let name = path.as_bytes();
        header[..name.len().min(100)].copy_from_slice(&name[..name.len().min(100)]);

        // Mode, uid, gid (octal strings)
        header[100..107].copy_from_slice(b"0000644");
        header[108..115].copy_from_slice(b"0000000");
        header[116..123].copy_from_slice(b"0000000");

        // Size (octal, 11 digits + null)
        let size_str = format!("{:011o}", content.len());
        header[124..135].copy_from_slice(size_str.as_bytes());

        // Mtime
        header[136..147].copy_from_slice(b"00000000000");

        // Typeflag: regular file, hardlink, symlink, etc.
        header[156] = typeflag;

        // Checksum: sum of all bytes with checksum field treated as spaces
        header[148..156].fill(b' ');
        let checksum: u32 = header.iter().map(|&b| b as u32).sum();
        let chk = format!("{:06o}\0 ", checksum);
        header[148..156].copy_from_slice(chk.as_bytes());

        // Assemble: header + data (padded to 512) + two zero end-of-archive blocks
        let mut tar_bytes = header.to_vec();
        let mut data = content.to_vec();
        let padded = (content.len() + 511) & !511;
        data.resize(padded, 0);
        tar_bytes.extend_from_slice(&data);
        tar_bytes.extend_from_slice(&[0u8; 1024]);

        let mut gz = Vec::new();
        let mut enc = flate2::write::GzEncoder::new(&mut gz, flate2::Compression::default());
        enc.write_all(&tar_bytes).unwrap();
        enc.finish().unwrap();
        gz
    }

    #[test]
    fn extract_rejects_path_traversal() {
        let dir = TempDir::new().unwrap();
        let bytes = make_raw_tar_gz("../escape", b"evil");
        let result = Store::extract_archive("evil-pack", &bytes, dir.path());
        assert!(result.is_err(), "path traversal entry should be rejected");
        assert!(
            !dir.path().parent().unwrap().join("escape").exists(),
            "traversal file must not be created outside the dest dir"
        );
    }

    #[test]
    fn extract_rejects_absolute_path() {
        let dir = TempDir::new().unwrap();
        let bytes = make_raw_tar_gz("/etc/evil", b"evil");
        let result = Store::extract_archive("evil-pack", &bytes, dir.path());
        assert!(result.is_err(), "absolute path entry should be rejected");
    }

    #[test]
    fn extract_nested_files_succeed() {
        let dir = TempDir::new().unwrap();
        let bytes = make_tar_gz(&[
            ("pack.toml", b"name = \"test\""),
            ("prompts/system.md", b"You are helpful."),
            ("commands/review.md", b"# Review"),
        ]);
        Store::extract_archive("test", &bytes, dir.path()).unwrap();
        assert!(dir.path().join("pack.toml").exists());
        assert!(dir.path().join("prompts/system.md").exists());
        assert!(dir.path().join("commands/review.md").exists());
    }

    #[test]
    fn extract_empty_archive_succeeds() {
        let dir = TempDir::new().unwrap();
        let bytes = make_tar_gz(&[]);
        assert!(Store::extract_archive("empty", &bytes, dir.path()).is_ok());
    }

    #[test]
    fn extract_rejects_symlink() {
        let dir = TempDir::new().unwrap();
        // typeflag b'2' = symbolic link
        let bytes = make_raw_tar_gz_with_type("innocent.md", b"", b'2');
        let result = Store::extract_archive("evil-pack", &bytes, dir.path());
        assert!(result.is_err(), "symlink entry should be rejected");
    }

    #[test]
    fn extract_rejects_hardlink() {
        let dir = TempDir::new().unwrap();
        // typeflag b'1' = hard link
        let bytes = make_raw_tar_gz_with_type("innocent.md", b"", b'1');
        let result = Store::extract_archive("evil-pack", &bytes, dir.path());
        assert!(result.is_err(), "hardlink entry should be rejected");
    }
}
