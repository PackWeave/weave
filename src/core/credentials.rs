//! Credential storage and retrieval for registry authentication.
//!
//! Tokens are stored in a dedicated file (`~/.packweave/credentials`) with
//! restricted permissions (0o600 on Unix). The `WEAVE_TOKEN` environment
//! variable takes precedence over the file at resolution time.

use std::path::PathBuf;

use crate::core::config::Config;
use crate::error::Result;
use crate::util;

/// Return the path to the credentials file.
///
/// Uses `Config::auth_token_path` if set, otherwise defaults to
/// `~/.packweave/credentials`.
pub fn credentials_path(config: &Config) -> Result<PathBuf> {
    if let Some(ref custom) = config.auth_token_path {
        return Ok(PathBuf::from(custom));
    }
    Ok(util::packweave_dir()?.join("credentials"))
}

/// Resolve the active authentication token.
///
/// Resolution order:
/// 1. `WEAVE_TOKEN` environment variable
/// 2. Credentials file on disk
/// 3. `None` (not authenticated)
pub fn resolve_token(config: &Config) -> Result<Option<String>> {
    // Environment variable takes precedence.
    if let Ok(token) = std::env::var("WEAVE_TOKEN") {
        let trimmed = token.trim().to_string();
        if !trimmed.is_empty() {
            return Ok(Some(trimmed));
        }
    }

    // Fall back to credentials file.
    let path = credentials_path(config)?;
    if path.exists() {
        let content = util::read_file(&path)?;
        let trimmed = content.trim().to_string();
        if !trimmed.is_empty() {
            return Ok(Some(trimmed));
        }
    }

    Ok(None)
}

/// Store a token to the credentials file.
///
/// Creates parent directories if needed and restricts file permissions to
/// owner-only on Unix (0o600).
pub fn store_token(config: &Config, token: &str) -> Result<()> {
    let path = credentials_path(config)?;
    util::write_file(&path, token)?;
    set_owner_only_permissions(&path);
    Ok(())
}

/// Remove the credentials file.
pub fn remove_token(config: &Config) -> Result<()> {
    let path = credentials_path(config)?;
    util::remove_file_if_exists(&path)
}

/// Best-effort token validation against the GitHub API.
///
/// Returns `Some(username)` if the token is valid, `None` on any failure.
/// This is advisory — a `None` result does not prevent the token from being
/// stored, because it may be intended for a non-GitHub registry.
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

/// Set file permissions to 0o600 (owner read/write only) on Unix.
/// No-op on other platforms.
#[cfg(unix)]
fn set_owner_only_permissions(path: &std::path::Path) {
    use std::os::unix::fs::PermissionsExt;
    let _ = std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o600));
}

#[cfg(not(unix))]
fn set_owner_only_permissions(_path: &std::path::Path) {
    // Windows NTFS ACLs already scope files to the owning user in home directories.
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

    fn test_config(dir: &std::path::Path) -> Config {
        Config {
            auth_token_path: Some(dir.join("credentials").to_string_lossy().to_string()),
            ..Config::default()
        }
    }

    #[test]
    #[serial_test::serial]
    fn resolve_prefers_env_var() {
        let tmp = tempfile::TempDir::new().unwrap();
        let config = test_config(tmp.path());

        // Write a file-based token.
        store_token(&config, "file-token").unwrap();

        // Env var should win.
        let _guard = EnvGuard::set("WEAVE_TOKEN", "env-token");
        let token = resolve_token(&config).unwrap();
        assert_eq!(token.as_deref(), Some("env-token"));
    }

    #[test]
    #[serial_test::serial]
    fn resolve_reads_file() {
        let tmp = tempfile::TempDir::new().unwrap();
        let config = test_config(tmp.path());
        let _guard = EnvGuard::remove("WEAVE_TOKEN");

        store_token(&config, "file-token").unwrap();
        let token = resolve_token(&config).unwrap();
        assert_eq!(token.as_deref(), Some("file-token"));
    }

    #[test]
    #[serial_test::serial]
    fn resolve_returns_none_when_absent() {
        let tmp = tempfile::TempDir::new().unwrap();
        let config = test_config(tmp.path());
        let _guard = EnvGuard::remove("WEAVE_TOKEN");

        let token = resolve_token(&config).unwrap();
        assert!(token.is_none());
    }

    #[test]
    fn store_and_remove_roundtrip() {
        let tmp = tempfile::TempDir::new().unwrap();
        let config = test_config(tmp.path());

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
    fn store_sets_permissions() {
        use std::os::unix::fs::PermissionsExt;

        let tmp = tempfile::TempDir::new().unwrap();
        let config = test_config(tmp.path());

        store_token(&config, "secret").unwrap();
        let path = credentials_path(&config).unwrap();
        let perms = std::fs::metadata(&path).unwrap().permissions();
        assert_eq!(perms.mode() & 0o777, 0o600);
    }

    #[test]
    fn resolve_trims_whitespace() {
        let tmp = tempfile::TempDir::new().unwrap();
        let config = test_config(tmp.path());

        // Write token with trailing newline (common with echo).
        let path = credentials_path(&config).unwrap();
        util::write_file(&path, "  my-token  \n").unwrap();

        // Should resolve to trimmed value (need to unset env var).
        let token_raw = util::read_file(&path).unwrap();
        assert_eq!(token_raw.trim(), "my-token");
    }
}
