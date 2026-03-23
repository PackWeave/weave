//! Credential storage and retrieval for registry authentication.
//!
//! Tokens are stored in a dedicated file (`~/.packweave/credentials`) with
//! restricted permissions (0o600 on Unix). The `WEAVE_TOKEN` environment
//! variable takes precedence over the file at resolution time.
//!
//! ## Security properties
//!
//! - Credentials file is created with 0o600 before writing content (no TOCTOU window)
//! - Symlinks are rejected on both read and write paths
//! - Token is validated for printable ASCII (no header injection)
//! - Token is only sent to the official registry, never to community taps

use std::path::PathBuf;

use crate::core::config::Config;
use crate::error::{Result, WeaveError};
use crate::util;

/// Where a resolved token came from.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TokenSource {
    EnvVar,
    File(PathBuf),
}

/// A resolved token together with its source.
#[derive(Debug, Clone)]
pub struct ResolvedToken {
    pub token: String,
    pub source: TokenSource,
}

/// Return the path to the credentials file.
///
/// Uses `Config::auth_token_path` if set (must be under `~/.packweave/`),
/// otherwise defaults to `~/.packweave/credentials`.
pub fn credentials_path(config: &Config) -> Result<PathBuf> {
    if let Some(ref custom) = config.auth_token_path {
        let custom_path = PathBuf::from(custom);
        // Validate the override is under the packweave directory to prevent
        // config.toml from redirecting credential reads/writes to arbitrary paths.
        // Skip this check in test builds (WEAVE_TEST_STORE_DIR overrides the
        // packweave dir to a temp directory).
        #[cfg(not(test))]
        {
            let packweave = util::packweave_dir()?;

            // Reject paths containing `..` components before attempting canonicalize,
            // because canonicalize falls back to raw comparison on non-existent paths
            // and `..` could bypass the containment check.
            if custom_path
                .components()
                .any(|c| matches!(c, std::path::Component::ParentDir))
            {
                return Err(WeaveError::io(
                    format!(
                        "auth_token_path '{}' contains '..' components — refusing to use it",
                        custom_path.display()
                    ),
                    std::io::Error::new(
                        std::io::ErrorKind::PermissionDenied,
                        "credential path must not contain '..' components",
                    ),
                ));
            }

            let canonical_custom = custom_path
                .canonicalize()
                .unwrap_or_else(|_| custom_path.clone());
            let canonical_packweave = packweave
                .canonicalize()
                .unwrap_or_else(|_| packweave.clone());
            if !canonical_custom.starts_with(&canonical_packweave) {
                return Err(WeaveError::io(
                    format!(
                        "auth_token_path '{}' is outside ~/.packweave/ — refusing to use it",
                        custom_path.display()
                    ),
                    std::io::Error::new(
                        std::io::ErrorKind::PermissionDenied,
                        "credential path must be under ~/.packweave/",
                    ),
                ));
            }
        }
        return Ok(custom_path);
    }
    Ok(util::packweave_dir()?.join("credentials"))
}

/// Resolve the active authentication token.
///
/// Resolution order:
/// 1. `WEAVE_TOKEN` environment variable
/// 2. Credentials file on disk (rejects symlinks)
/// 3. `None` (not authenticated)
pub fn resolve_token(config: &Config) -> Result<Option<ResolvedToken>> {
    // Environment variable takes precedence.
    if let Ok(token) = std::env::var("WEAVE_TOKEN") {
        let trimmed = token.trim().to_string();
        if !trimmed.is_empty() {
            if validate_token_format(&trimmed).is_err() {
                log::warn!("WEAVE_TOKEN contains invalid characters — ignoring it");
                // Fall through to file-based resolution.
            } else {
                return Ok(Some(ResolvedToken {
                    token: trimmed,
                    source: TokenSource::EnvVar,
                }));
            }
        }
    }

    // Fall back to credentials file.
    let path = credentials_path(config)?;
    if path.exists() {
        reject_symlink(&path)?;
        let content = util::read_file(&path)?;
        let trimmed = content.trim().to_string();
        if !trimmed.is_empty() {
            if validate_token_format(&trimmed).is_err() {
                log::warn!(
                    "credentials file '{}' contains invalid characters — ignoring it",
                    path.display()
                );
                return Ok(None);
            }
            return Ok(Some(ResolvedToken {
                token: trimmed,
                source: TokenSource::File(path),
            }));
        }
    }

    Ok(None)
}

/// Validate that a token contains only printable ASCII characters.
///
/// Rejects tokens with newlines, control characters, or non-ASCII bytes that
/// could cause header injection or other unexpected behavior.
pub fn validate_token_format(token: &str) -> Result<()> {
    if token.is_empty() {
        return Err(WeaveError::io(
            "validating token",
            std::io::Error::new(std::io::ErrorKind::InvalidInput, "token cannot be empty"),
        ));
    }
    if !token.bytes().all(|b| b.is_ascii_graphic() || b == b' ') {
        return Err(WeaveError::io(
            "validating token",
            std::io::Error::new(
                std::io::ErrorKind::InvalidInput,
                "token contains non-printable or non-ASCII characters",
            ),
        ));
    }
    Ok(())
}

/// Store a token to the credentials file.
///
/// Creates parent directories if needed. On Unix, the file is created with
/// 0o600 permissions atomically (no TOCTOU window). Rejects symlinks at the
/// target path.
pub fn store_token(config: &Config, token: &str) -> Result<()> {
    validate_token_format(token)?;

    let path = credentials_path(config)?;

    // Reject symlinks: if the path exists and is a symlink, refuse to write.
    if path.exists() {
        reject_symlink(&path)?;
    }

    // Ensure parent directory exists.
    if let Some(parent) = path.parent() {
        util::ensure_dir(parent)?;
    }

    // On Unix: create a securely random temp file with 0o600, then atomically
    // rename into place. Uses tempfile crate to avoid predictable filenames
    // (a predictable path could be pre-created as a symlink by an attacker).
    #[cfg(unix)]
    {
        use std::io::Write;
        use std::os::unix::fs::PermissionsExt;

        let parent = path
            .parent()
            .expect("credentials file path must have a parent directory");
        let mut tmp = tempfile::Builder::new()
            .prefix(".credentials.")
            .tempfile_in(parent)
            .map_err(|e| WeaveError::io("creating temporary credentials file", e))?;

        // Set 0o600 before writing content via the open file handle.
        tmp.as_file()
            .set_permissions(std::fs::Permissions::from_mode(0o600))
            .map_err(|e| WeaveError::io("setting credentials file permissions", e))?;
        tmp.write_all(token.as_bytes())
            .map_err(|e| WeaveError::io("writing credentials", e))?;
        tmp.as_file()
            .sync_all()
            .map_err(|e| WeaveError::io("syncing credentials", e))?;

        tmp.persist(&path)
            .map_err(|e| WeaveError::io("finalizing credentials file", e.error))?;
    }

    // On non-Unix: write directly (NTFS ACLs protect the user's home directory).
    #[cfg(not(unix))]
    {
        util::write_file(&path, token)?;
    }

    Ok(())
}

/// Remove the credentials file.
pub fn remove_token(config: &Config) -> Result<()> {
    let path = credentials_path(config)?;
    if path.exists() {
        reject_symlink(&path)?;
    }
    util::remove_file_if_exists(&path)
}

/// Best-effort token validation against the GitHub API.
///
/// Returns `Some(username)` if the token is valid, `None` on any failure.
/// This is advisory — a `None` result does not prevent the token from being
/// stored, because it may be intended for a non-GitHub registry.
///
/// Only called when the registry URL points to GitHub (raw.githubusercontent.com).
pub fn validate_github_token(token: &str) -> Option<String> {
    let client = reqwest::blocking::Client::new();
    let response = client
        .get("https://api.github.com/user")
        .header("User-Agent", format!("weave/{}", env!("CARGO_PKG_VERSION")))
        .header("Authorization", format!("Bearer {token}"))
        .send()
        .ok()?;

    if !response.status().is_success() {
        return None;
    }

    let body: serde_json::Value = response.json().ok()?;
    body["login"].as_str().map(|s| s.to_string())
}

/// Returns true if the registry URL points to GitHub (the default registry).
pub fn is_github_registry(registry_url: &str) -> bool {
    let host_with_port = registry_url
        .split("://")
        .nth(1)
        .and_then(|r| r.split('/').next())
        .unwrap_or("");
    let host = host_with_port.split(':').next().unwrap_or(host_with_port);

    const GITHUB_DOMAINS: [&str; 2] = ["github.com", "githubusercontent.com"];
    GITHUB_DOMAINS.iter().any(|domain| {
        host == *domain
            || (host.ends_with(domain)
                && host.as_bytes().get(host.len() - domain.len() - 1) == Some(&b'.'))
    })
}

/// Reject a path if it is a symlink.
fn reject_symlink(path: &std::path::Path) -> Result<()> {
    let meta =
        std::fs::symlink_metadata(path).map_err(|e| WeaveError::io("checking credentials", e))?;
    if meta.file_type().is_symlink() {
        return Err(WeaveError::io(
            format!(
                "credentials path '{}' is a symlink — refusing for security",
                path.display()
            ),
            std::io::Error::new(
                std::io::ErrorKind::PermissionDenied,
                "symlink credentials file rejected",
            ),
        ));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// RAII guard that sets an env var on creation and restores it on drop.
    struct EnvGuard {
        key: &'static str,
        prev: Option<String>,
    }

    impl EnvGuard {
        fn set(key: &'static str, value: &str) -> Self {
            let prev = std::env::var(key).ok();
            unsafe { std::env::set_var(key, value) };
            Self { key, prev }
        }

        fn remove(key: &'static str) -> Self {
            let prev = std::env::var(key).ok();
            unsafe { std::env::remove_var(key) };
            Self { key, prev }
        }
    }

    impl Drop for EnvGuard {
        fn drop(&mut self) {
            match &self.prev {
                Some(v) => unsafe { std::env::set_var(self.key, v) },
                None => unsafe { std::env::remove_var(self.key) },
            }
        }
    }

    /// Create a test config with credentials path under a temp dir.
    /// Also sets WEAVE_TEST_STORE_DIR so path validation passes.
    fn test_config_with_guard(tmp: &tempfile::TempDir) -> (Config, EnvGuard) {
        let guard = EnvGuard::set("WEAVE_TEST_STORE_DIR", &tmp.path().to_string_lossy());
        let config = Config {
            auth_token_path: Some(tmp.path().join("credentials").to_string_lossy().to_string()),
            ..Config::default()
        };
        (config, guard)
    }

    #[test]
    #[serial_test::serial]
    fn resolve_prefers_env_var() {
        let tmp = tempfile::TempDir::new().unwrap();
        let (config, _store_guard) = test_config_with_guard(&tmp);

        // Write a file-based token.
        store_token(&config, "file-token").unwrap();

        // Env var should win.
        let _guard = EnvGuard::set("WEAVE_TOKEN", "env-token");
        let resolved = resolve_token(&config).unwrap();
        assert_eq!(
            resolved.as_ref().map(|r| r.token.as_str()),
            Some("env-token")
        );
        assert_eq!(
            resolved.as_ref().map(|r| &r.source),
            Some(&TokenSource::EnvVar)
        );
    }

    #[test]
    #[serial_test::serial]
    fn resolve_reads_file() {
        let tmp = tempfile::TempDir::new().unwrap();
        let (config, _store_guard) = test_config_with_guard(&tmp);
        let _guard = EnvGuard::remove("WEAVE_TOKEN");

        store_token(&config, "file-token").unwrap();
        let resolved = resolve_token(&config).unwrap();
        assert_eq!(
            resolved.as_ref().map(|r| r.token.as_str()),
            Some("file-token")
        );
        assert!(matches!(
            resolved.as_ref().map(|r| &r.source),
            Some(TokenSource::File(_))
        ));
    }

    #[test]
    #[serial_test::serial]
    fn resolve_returns_none_when_absent() {
        let tmp = tempfile::TempDir::new().unwrap();
        let (config, _store_guard) = test_config_with_guard(&tmp);
        let _guard = EnvGuard::remove("WEAVE_TOKEN");

        let token = resolve_token(&config).unwrap();
        assert!(token.is_none());
    }

    #[test]
    #[serial_test::serial]
    fn store_and_remove_roundtrip() {
        let tmp = tempfile::TempDir::new().unwrap();
        let (config, _store_guard) = test_config_with_guard(&tmp);

        store_token(&config, "my-secret-token").unwrap();
        let path = credentials_path(&config).unwrap();
        assert!(path.exists());

        let content = std::fs::read_to_string(&path).unwrap();
        assert_eq!(content, "my-secret-token");

        remove_token(&config).unwrap();
        assert!(!path.exists());
    }

    #[cfg(unix)]
    #[test]
    #[serial_test::serial]
    fn store_sets_permissions_atomically() {
        use std::os::unix::fs::PermissionsExt;

        let tmp = tempfile::TempDir::new().unwrap();
        let (config, _store_guard) = test_config_with_guard(&tmp);

        store_token(&config, "secret").unwrap();
        let path = credentials_path(&config).unwrap();
        let perms = std::fs::metadata(&path).unwrap().permissions();
        assert_eq!(perms.mode() & 0o777, 0o600);
    }

    #[test]
    #[serial_test::serial]
    fn resolve_trims_whitespace() {
        let tmp = tempfile::TempDir::new().unwrap();
        let (config, _store_guard) = test_config_with_guard(&tmp);
        let _guard = EnvGuard::remove("WEAVE_TOKEN");

        // Write token with trailing newline (common with echo).
        let path = credentials_path(&config).unwrap();
        util::write_file(&path, "  my-token  \n").unwrap();

        // resolve_token should return the trimmed value.
        let resolved = resolve_token(&config).unwrap();
        assert_eq!(
            resolved.as_ref().map(|r| r.token.as_str()),
            Some("my-token")
        );
    }

    #[test]
    fn validate_token_rejects_empty() {
        assert!(validate_token_format("").is_err());
    }

    #[test]
    fn validate_token_rejects_newlines() {
        assert!(validate_token_format("ghp_xxxx\r\nEvil: header").is_err());
    }

    #[test]
    fn validate_token_rejects_control_chars() {
        assert!(validate_token_format("ghp_xxxx\x00").is_err());
    }

    #[test]
    fn validate_token_accepts_valid_pat() {
        assert!(validate_token_format("ghp_ABCdef1234567890abcdef1234567890abcd").is_ok());
    }

    #[test]
    fn is_github_registry_detects_default() {
        assert!(is_github_registry(
            "https://raw.githubusercontent.com/PackWeave/registry/main"
        ));
    }

    #[test]
    fn is_github_registry_detects_custom_github() {
        assert!(is_github_registry("https://github.com/my-org/registry"));
    }

    #[test]
    fn is_github_registry_rejects_non_github() {
        assert!(!is_github_registry("https://my-registry.example.com"));
    }

    #[test]
    fn is_github_registry_rejects_substring_match() {
        assert!(!is_github_registry("https://evil-github.com/repo"));
    }

    #[test]
    fn is_github_registry_accepts_subdomains() {
        // Subdomains of githubusercontent.com are legitimate GitHub hosts.
        assert!(is_github_registry(
            "https://objects.githubusercontent.com/repo"
        ));
    }

    #[cfg(unix)]
    #[test]
    #[serial_test::serial]
    fn reject_symlink_credentials() {
        let tmp = tempfile::TempDir::new().unwrap();
        let _store_guard = EnvGuard::set("WEAVE_TEST_STORE_DIR", &tmp.path().to_string_lossy());

        let target = tmp.path().join("real-file");
        let link = tmp.path().join("credentials");

        std::fs::write(&target, "token").unwrap();
        std::os::unix::fs::symlink(&target, &link).unwrap();

        let config = Config {
            auth_token_path: Some(link.to_string_lossy().to_string()),
            ..Config::default()
        };

        // resolve_token should reject the symlink.
        let _guard = EnvGuard::remove("WEAVE_TOKEN");
        let result = resolve_token(&config);
        assert!(result.is_err());
        assert!(
            result.unwrap_err().to_string().contains("symlink"),
            "error should mention symlink"
        );
    }
}
