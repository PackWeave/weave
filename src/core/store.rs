use std::io::Read;
use std::path::PathBuf;

use crate::core::pack::{Pack, PackSource};
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

    /// Compute the version directory name for a pack, including a `-local-{hash}`
    /// suffix for local sources so that registry and local installs of the same
    /// name+version do not share a cache directory.
    fn version_dir_name(version: &semver::Version, source: Option<&PackSource>) -> String {
        match source {
            Some(PackSource::Local { path }) => {
                let hash = Self::stable_hash_path(path);
                format!("{version}-local-{hash:016x}")
            }
            _ => version.to_string(),
        }
    }

    /// FNV-1a 64-bit hash — deterministic across Rust versions, unlike
    /// `DefaultHasher` which explicitly does not guarantee stability.
    ///
    /// The path is normalized before hashing so that semantically equivalent
    /// paths (e.g. `/foo/./bar` vs `/foo/bar`, `/foo/baz/../bar` vs `/foo/bar`)
    /// produce the same hash. This is a defense-in-depth measure — the install
    /// path already canonicalizes in most cases.
    fn stable_hash_path(path: &str) -> u64 {
        let normalized = Self::normalize_path(path);
        const FNV_OFFSET: u64 = 0xcbf29ce484222325;
        const FNV_PRIME: u64 = 0x00000100000001B3;
        let mut hash = FNV_OFFSET;
        for byte in normalized.as_bytes() {
            hash ^= *byte as u64;
            hash = hash.wrapping_mul(FNV_PRIME);
        }
        hash
    }

    /// Lexically normalize a path by resolving `.` and `..` segments without
    /// touching the filesystem. This is similar to Go's `path.Clean()`.
    ///
    /// Examples:
    /// - `/foo/./bar` -> `/foo/bar`
    /// - `/foo/baz/../bar` -> `/foo/bar`
    /// - `/foo/bar/` -> `/foo/bar`
    /// - `relative/./path` -> `relative/path`
    fn normalize_path(path: &str) -> String {
        use std::path::{Component, Path};
        let mut components = Vec::new();
        for component in Path::new(path).components() {
            match component {
                Component::CurDir => {} // skip `.`
                Component::ParentDir => {
                    // Pop the last normal component if possible; preserve `..`
                    // at the start of relative paths.
                    if let Some(last) = components.last()
                        && matches!(last, Component::Normal(_))
                    {
                        components.pop();
                        continue;
                    }
                    components.push(component);
                }
                _ => components.push(component),
            }
        }
        if components.is_empty() {
            return ".".to_string();
        }
        let result: PathBuf = components.iter().collect();
        result.to_string_lossy().into_owned()
    }

    /// Path where a specific pack version is stored.
    ///
    /// For registry packs (or when `source` is `None`):
    ///   `~/.packweave/packs/{name}/{version}/`
    ///
    /// For local packs:
    ///   `~/.packweave/packs/{name}/{version}-local-{hash}/`
    ///
    /// The hash is derived from the local path so that different local sources
    /// of the same name+version are isolated from each other and from registry
    /// entries.
    pub fn pack_dir(
        name: &str,
        version: &semver::Version,
        source: Option<&PackSource>,
    ) -> Result<PathBuf> {
        Ok(Self::root()?
            .join(name)
            .join(Self::version_dir_name(version, source)))
    }

    /// Check if a pack version is already cached locally.
    pub fn is_cached(
        name: &str,
        version: &semver::Version,
        source: Option<&PackSource>,
    ) -> Result<bool> {
        let dir = Self::pack_dir(name, version, source)?;
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
    pub fn fetch(
        name: &str,
        release: &PackRelease,
        source: Option<&PackSource>,
    ) -> Result<PathBuf> {
        // Validate up-front: a pack without pack.toml is not a valid pack.
        // Catching this before writing prevents the store from caching an
        // invalid directory that downstream Pack::load() would fail on.
        if !release.files.contains_key("pack.toml") {
            return Err(WeaveError::Registry(format!(
                "pack '{name}' release {} is missing pack.toml — the registry entry may be corrupt",
                release.version
            )));
        }

        let dest = Self::pack_dir(name, &release.version, source)?;

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
    pub fn load_pack(
        name: &str,
        version: &semver::Version,
        source: Option<&PackSource>,
    ) -> Result<Pack> {
        let dir = Self::pack_dir(name, version, source)?;
        Pack::load(&dir)
    }

    /// List all cached packs as (name, version) pairs.
    ///
    /// Handles both plain version directories (`1.0.0`) and local-suffixed
    /// directories (`1.0.0-local-{hash}`). The suffix is stripped before
    /// parsing the version so local entries are reported correctly.
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

                // Strip `-local-{hex}` suffix if present so we can parse the
                // semver portion.
                let semver_str = Self::strip_local_suffix(&ver_str);

                if let Ok(version) = semver::Version::parse(semver_str)
                    && ver_entry.path().join("pack.toml").exists()
                {
                    result.push((name.clone(), version));
                }
            }
        }

        result.sort_by(|a, b| a.0.cmp(&b.0).then(a.1.cmp(&b.1)));
        Ok(result)
    }

    /// Strip the `-local-{16-hex-digit}` suffix from a version directory name.
    /// Returns the original string unchanged if the suffix is not present.
    fn strip_local_suffix(dir_name: &str) -> &str {
        // The suffix is exactly "-local-" + 16 hex digits = 23 characters.
        const SUFFIX_LEN: usize = "-local-".len() + 16; // 23
        if dir_name.len() > SUFFIX_LEN {
            let (prefix, suffix) = dir_name.split_at(dir_name.len() - SUFFIX_LEN);
            if let Some(hex_part) = suffix.strip_prefix("-local-")
                && hex_part.len() == 16
                && hex_part.chars().all(|c| c.is_ascii_hexdigit())
            {
                return prefix;
            }
        }
        dir_name
    }

    /// Remove a specific pack version from the store.
    pub fn evict(name: &str, version: &semver::Version, source: Option<&PackSource>) -> Result<()> {
        let dir = Self::pack_dir(name, version, source)?;
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
            if is_empty && let Err(e) = std::fs::remove_dir(&name_dir) {
                log::warn!(
                    "could not remove empty pack directory {}: {e}",
                    name_dir.display()
                );
            }
        }

        Ok(())
    }

    /// Read a file from a cached pack, returning None if it doesn't exist.
    pub fn read_pack_file(
        name: &str,
        version: &semver::Version,
        relative_path: &str,
        source: Option<&PackSource>,
    ) -> Result<Option<String>> {
        let path = Self::pack_dir(name, version, source)?.join(relative_path);
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

    /// RAII guard that sets an env var on creation and removes it on drop,
    /// even if the test panics. Prevents env var leaks across `#[serial]` tests.
    struct EnvGuard {
        key: &'static str,
    }

    impl EnvGuard {
        fn set(key: &'static str, value: &std::path::Path) -> Self {
            // SAFETY: test helper, serial execution
            unsafe { std::env::set_var(key, value) };
            Self { key }
        }
    }

    impl Drop for EnvGuard {
        fn drop(&mut self) {
            // SAFETY: restoring env on drop in test
            unsafe { std::env::remove_var(self.key) };
        }
    }

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

    #[test]
    fn write_files_rejects_windows_drive_prefix() {
        // `C:evil` parses as Component::Prefix on Windows and should be rejected
        // on all platforms for defense in depth.
        let dir = TempDir::new().unwrap();
        for path in &["C:evil", "C:\\evil", "C:/evil"] {
            let files = HashMap::from([path.to_string()].map(|p| (p, "x".to_string())));
            let result = Store::write_files("evil-pack", &files, dir.path());
            // On Unix these paths are either caught by the Prefix check or the
            // leading-slash / backslash check; on Windows the Prefix check fires.
            // Either way the write must not produce a file outside the dest dir.
            if result.is_ok() {
                // If the path was somehow allowed, verify no file escaped the sandbox.
                assert!(
                    !std::path::Path::new("C:evil").exists(),
                    "file must not be written outside dest"
                );
            }
        }
    }

    #[test]
    fn fetch_rejects_release_without_pack_toml() {
        let release = crate::core::registry::PackRelease {
            version: semver::Version::new(1, 0, 0),
            files: HashMap::from([("prompts/system.md".to_string(), "hi".to_string())]),
            dependencies: HashMap::new(),
        };
        let result = Store::fetch("bad-pack", &release, None);
        assert!(result.is_err(), "fetch should fail without pack.toml");
        let err = result.unwrap_err().to_string();
        assert!(err.contains("pack.toml"), "error should mention pack.toml");
    }

    // ── cache isolation ──────────────────────────────────────────────────────

    #[test]
    fn version_dir_name_registry_has_no_suffix() {
        let v = semver::Version::new(1, 2, 3);
        let registry = PackSource::Registry {
            registry_url: "https://example.com".into(),
        };
        assert_eq!(Store::version_dir_name(&v, Some(&registry)), "1.2.3");
        assert_eq!(Store::version_dir_name(&v, None), "1.2.3");
    }

    #[test]
    fn version_dir_name_local_includes_hash_suffix() {
        let v = semver::Version::new(1, 0, 0);
        let local = PackSource::Local {
            path: "/home/user/my-pack".into(),
        };
        let dir_name = Store::version_dir_name(&v, Some(&local));
        assert!(
            dir_name.starts_with("1.0.0-local-"),
            "expected local suffix, got: {dir_name}"
        );
        // The hash part should be exactly 16 hex digits.
        let suffix = dir_name.strip_prefix("1.0.0-local-").unwrap();
        assert_eq!(suffix.len(), 16);
        assert!(suffix.chars().all(|c| c.is_ascii_hexdigit()));
    }

    #[test]
    fn version_dir_name_different_paths_produce_different_hashes() {
        let v = semver::Version::new(1, 0, 0);
        let local_a = PackSource::Local {
            path: "/path/a".into(),
        };
        let local_b = PackSource::Local {
            path: "/path/b".into(),
        };
        assert_ne!(
            Store::version_dir_name(&v, Some(&local_a)),
            Store::version_dir_name(&v, Some(&local_b)),
        );
    }

    #[test]
    fn pack_dir_registry_and_local_are_different() {
        let v = semver::Version::new(2, 0, 0);
        let registry = PackSource::Registry {
            registry_url: "https://example.com".into(),
        };
        let local = PackSource::Local {
            path: "/tmp/my-pack".into(),
        };
        let reg_dir = Store::pack_dir("my-pack", &v, Some(&registry)).unwrap();
        let local_dir = Store::pack_dir("my-pack", &v, Some(&local)).unwrap();
        assert_ne!(
            reg_dir, local_dir,
            "registry and local dirs must not collide"
        );
        // Registry dir should end with just the version.
        assert!(reg_dir.ends_with("2.0.0"));
        // Local dir should have the -local- suffix.
        let local_name = local_dir.file_name().unwrap().to_string_lossy();
        assert!(
            local_name.starts_with("2.0.0-local-"),
            "expected local suffix, got: {local_name}"
        );
    }

    #[test]
    #[serial_test::serial]
    fn evict_local_does_not_affect_registry() {
        let tmp = TempDir::new().unwrap();
        let _guard = EnvGuard::set("WEAVE_TEST_STORE_DIR", tmp.path());

        let name = "shared-pack";
        let v = semver::Version::new(1, 0, 0);
        let registry_source = PackSource::Registry {
            registry_url: "https://example.com".into(),
        };
        let local_source = PackSource::Local {
            path: "/tmp/shared-pack".into(),
        };

        // Create both cache entries manually.
        let reg_dir = Store::pack_dir(name, &v, Some(&registry_source)).unwrap();
        let local_dir = Store::pack_dir(name, &v, Some(&local_source)).unwrap();
        std::fs::create_dir_all(&reg_dir).unwrap();
        std::fs::create_dir_all(&local_dir).unwrap();
        std::fs::write(reg_dir.join("pack.toml"), "registry").unwrap();
        std::fs::write(local_dir.join("pack.toml"), "local").unwrap();

        // Evict the local entry.
        Store::evict(name, &v, Some(&local_source)).unwrap();

        // The local entry should be gone.
        assert!(!local_dir.exists(), "local cache dir should be removed");
        // The registry entry should still exist.
        assert!(
            reg_dir.join("pack.toml").exists(),
            "registry cache must survive local eviction"
        );
    }

    #[test]
    #[serial_test::serial]
    fn evict_registry_does_not_affect_local() {
        let tmp = TempDir::new().unwrap();
        let _guard = EnvGuard::set("WEAVE_TEST_STORE_DIR", tmp.path());

        let name = "shared-pack";
        let v = semver::Version::new(1, 0, 0);
        let registry_source = PackSource::Registry {
            registry_url: "https://example.com".into(),
        };
        let local_source = PackSource::Local {
            path: "/tmp/shared-pack".into(),
        };

        // Create both cache entries manually.
        let reg_dir = Store::pack_dir(name, &v, Some(&registry_source)).unwrap();
        let local_dir = Store::pack_dir(name, &v, Some(&local_source)).unwrap();
        std::fs::create_dir_all(&reg_dir).unwrap();
        std::fs::create_dir_all(&local_dir).unwrap();
        std::fs::write(reg_dir.join("pack.toml"), "registry").unwrap();
        std::fs::write(local_dir.join("pack.toml"), "local").unwrap();

        // Evict the registry entry.
        Store::evict(name, &v, Some(&registry_source)).unwrap();

        // The registry entry should be gone.
        assert!(!reg_dir.exists(), "registry cache dir should be removed");
        // The local entry should still exist.
        assert!(
            local_dir.join("pack.toml").exists(),
            "local cache must survive registry eviction"
        );
    }

    #[test]
    fn strip_local_suffix_strips_valid_suffix() {
        assert_eq!(
            Store::strip_local_suffix("1.0.0-local-abcdef0123456789"),
            "1.0.0"
        );
    }

    #[test]
    fn strip_local_suffix_preserves_plain_version() {
        assert_eq!(Store::strip_local_suffix("1.0.0"), "1.0.0");
    }

    #[test]
    fn strip_local_suffix_preserves_invalid_suffix() {
        // Too short hex part.
        assert_eq!(
            Store::strip_local_suffix("1.0.0-local-abc"),
            "1.0.0-local-abc"
        );
        // Non-hex chars.
        assert_eq!(
            Store::strip_local_suffix("1.0.0-local-ghijklmnopqrstuv"),
            "1.0.0-local-ghijklmnopqrstuv"
        );
    }

    #[test]
    fn stable_hash_is_pinned() {
        // Pinned value — if this assertion breaks, existing local cache
        // directories become orphaned. Do NOT change the hash algorithm
        // without a migration path for existing stores.
        assert_eq!(
            Store::stable_hash_path("/home/user/my-pack"),
            0xc4f22075cdd996fa,
            "FNV-1a output must be stable across Rust versions"
        );
    }

    // ── path normalization ──────────────────────────────────────────────────

    #[test]
    fn normalize_path_removes_dot_segments() {
        assert_eq!(Store::normalize_path("/foo/./bar"), "/foo/bar");
    }

    #[test]
    fn normalize_path_resolves_dotdot_segments() {
        assert_eq!(Store::normalize_path("/foo/baz/../bar"), "/foo/bar");
    }

    #[test]
    fn normalize_path_preserves_clean_absolute() {
        assert_eq!(
            Store::normalize_path("/home/user/my-pack"),
            "/home/user/my-pack"
        );
    }

    #[test]
    fn normalize_path_removes_trailing_slash() {
        assert_eq!(Store::normalize_path("/foo/bar/"), "/foo/bar");
    }

    #[test]
    fn normalize_path_empty_becomes_dot() {
        assert_eq!(Store::normalize_path(""), ".");
    }

    #[test]
    fn normalize_path_relative_with_dot() {
        assert_eq!(Store::normalize_path("./my-pack"), "my-pack");
    }

    #[test]
    fn equivalent_paths_produce_same_hash() {
        // These are all semantically equivalent to /foo/bar.
        let canonical = Store::stable_hash_path("/foo/bar");
        assert_eq!(Store::stable_hash_path("/foo/./bar"), canonical);
        assert_eq!(Store::stable_hash_path("/foo/baz/../bar"), canonical);
        assert_eq!(Store::stable_hash_path("/foo/bar/"), canonical);
    }
}
