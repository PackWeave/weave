use std::path::PathBuf;

use serde::{Deserialize, Serialize};

use crate::error::{Result, WeaveError};
use crate::util;

/// A community tap — a GitHub repository following the same registry index format.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct TapConfig {
    /// The tap identifier in `user/repo` format.
    pub name: String,
    /// The base URL for the tap's registry index.
    pub url: String,
}

/// Global weave configuration stored at `~/.packweave/config.toml`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Config {
    #[serde(default = "default_active_profile")]
    pub active_profile: String,
    #[serde(default = "default_registry_url")]
    pub registry_url: String,
    #[serde(default)]
    pub auth_token_path: Option<String>,
    #[serde(default)]
    pub taps: Vec<TapConfig>,
}

fn default_active_profile() -> String {
    "default".into()
}

fn default_registry_url() -> String {
    "https://raw.githubusercontent.com/PackWeave/registry/main".into()
}

impl Default for Config {
    fn default() -> Self {
        Self {
            active_profile: default_active_profile(),
            registry_url: default_registry_url(),
            auth_token_path: None,
            taps: Vec::new(),
        }
    }
}

impl Config {
    /// Path to the config file: `~/.packweave/config.toml`
    pub fn path() -> Result<PathBuf> {
        Ok(util::packweave_dir()?.join("config.toml"))
    }

    /// Load config from disk, returning defaults if file doesn't exist.
    ///
    /// The `WEAVE_REGISTRY_URL` environment variable, when set, overrides the
    /// `registry_url` from disk. This is used by E2E tests to point the CLI at
    /// a mock registry without touching real config files.
    pub fn load() -> Result<Self> {
        let path = Self::path()?;
        let mut config = if !path.exists() {
            Self::default()
        } else {
            let content = util::read_file(&path)?;
            toml::from_str(&content).map_err(|e| crate::error::WeaveError::Toml {
                path,
                source: Box::new(e),
            })?
        };
        if let Ok(url) = std::env::var("WEAVE_REGISTRY_URL") {
            config.registry_url = url;
        }
        Ok(config)
    }

    /// Save config to disk.
    pub fn save(&self) -> Result<()> {
        let path = Self::path()?;
        // Config fields are all simple types — TOML serialization cannot fail.
        let content = toml::to_string_pretty(self).expect("Config serialization cannot fail");
        util::write_file(&path, &content)
    }

    /// Register a community tap by `user/repo` name.
    ///
    /// The tap URL is constructed as `https://raw.githubusercontent.com/{user}/{repo}/main`.
    pub fn add_tap(&mut self, name: &str) -> Result<()> {
        validate_tap_name(name)?;
        if self.taps.iter().any(|t| t.name == name) {
            return Err(WeaveError::TapAlreadyExists {
                name: name.to_string(),
            });
        }
        self.taps.push(TapConfig {
            name: name.to_string(),
            url: tap_url(name),
        });
        Ok(())
    }

    /// Deregister a community tap by `user/repo` name.
    pub fn remove_tap(&mut self, name: &str) -> Result<()> {
        validate_tap_name(name)?;
        let len_before = self.taps.len();
        self.taps.retain(|t| t.name != name);
        if self.taps.len() == len_before {
            return Err(WeaveError::TapNotFound {
                name: name.to_string(),
            });
        }
        Ok(())
    }

    /// Return the list of registered taps.
    pub fn list_taps(&self) -> &[TapConfig] {
        &self.taps
    }
}

/// Validate that a tap name is in `user/repo` format.
///
/// Both segments must be non-empty and contain only alphanumeric characters,
/// hyphens, underscores, or dots.
pub fn validate_tap_name(name: &str) -> Result<()> {
    let parts: Vec<&str> = name.split('/').collect();
    if parts.len() != 2 {
        return Err(WeaveError::InvalidTapName {
            name: name.to_string(),
        });
    }
    let valid_segment = |s: &str| {
        !s.is_empty()
            && s.chars()
                .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_' || c == '.')
    };
    if !valid_segment(parts[0]) || !valid_segment(parts[1]) {
        return Err(WeaveError::InvalidTapName {
            name: name.to_string(),
        });
    }
    Ok(())
}

/// Construct the raw GitHub URL for a tap's registry index.
fn tap_url(name: &str) -> String {
    format!("https://raw.githubusercontent.com/{name}/main")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_config_values() {
        let config = Config::default();
        assert_eq!(config.active_profile, "default");
        assert!(config.registry_url.contains("PackWeave"));
        assert!(config.auth_token_path.is_none());
        assert!(config.taps.is_empty());
    }

    #[test]
    fn roundtrip_toml() {
        let config = Config::default();
        let toml_str = toml::to_string_pretty(&config).expect("serialize");
        let parsed: Config = toml::from_str(&toml_str).expect("deserialize");
        assert_eq!(parsed.active_profile, config.active_profile);
        assert_eq!(parsed.registry_url, config.registry_url);
    }

    #[test]
    fn roundtrip_toml_with_taps() {
        let mut config = Config::default();
        config.add_tap("acme/my-packs").unwrap();
        let toml_str = toml::to_string_pretty(&config).expect("serialize");
        let parsed: Config = toml::from_str(&toml_str).expect("deserialize");
        assert_eq!(parsed.taps.len(), 1);
        assert_eq!(parsed.taps[0].name, "acme/my-packs");
        assert!(parsed.taps[0].url.contains("acme/my-packs"));
    }

    #[test]
    fn add_tap_valid() {
        let mut config = Config::default();
        config.add_tap("acme/my-packs").unwrap();
        assert_eq!(config.taps.len(), 1);
        assert_eq!(config.taps[0].name, "acme/my-packs");
        assert_eq!(
            config.taps[0].url,
            "https://raw.githubusercontent.com/acme/my-packs/main"
        );
    }

    #[test]
    fn add_tap_duplicate_errors() {
        let mut config = Config::default();
        config.add_tap("acme/my-packs").unwrap();
        let err = config.add_tap("acme/my-packs").unwrap_err();
        assert!(err.to_string().contains("already registered"));
    }

    #[test]
    fn remove_tap_valid() {
        let mut config = Config::default();
        config.add_tap("acme/my-packs").unwrap();
        config.remove_tap("acme/my-packs").unwrap();
        assert!(config.taps.is_empty());
    }

    #[test]
    fn remove_tap_not_found() {
        let mut config = Config::default();
        let err = config.remove_tap("acme/my-packs").unwrap_err();
        assert!(err.to_string().contains("not registered"));
    }

    #[test]
    fn list_taps_returns_registered() {
        let mut config = Config::default();
        config.add_tap("acme/packs-a").unwrap();
        config.add_tap("other/packs-b").unwrap();
        let taps = config.list_taps();
        assert_eq!(taps.len(), 2);
        assert_eq!(taps[0].name, "acme/packs-a");
        assert_eq!(taps[1].name, "other/packs-b");
    }

    #[test]
    fn validate_tap_name_invalid_formats() {
        // No slash
        assert!(validate_tap_name("noslash").is_err());
        // Too many slashes
        assert!(validate_tap_name("a/b/c").is_err());
        // Empty segments
        assert!(validate_tap_name("/repo").is_err());
        assert!(validate_tap_name("user/").is_err());
        // Invalid chars
        assert!(validate_tap_name("user/repo with spaces").is_err());
    }

    #[test]
    fn validate_tap_name_valid_formats() {
        assert!(validate_tap_name("user/repo").is_ok());
        assert!(validate_tap_name("my-org/my-packs").is_ok());
        assert!(validate_tap_name("user_1/repo.v2").is_ok());
    }
}
