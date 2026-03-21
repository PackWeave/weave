use std::path::PathBuf;

use serde::{Deserialize, Serialize};

use crate::error::Result;
use crate::util;

/// Global weave configuration stored at `~/.packweave/config.toml`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Config {
    #[serde(default = "default_active_profile")]
    pub active_profile: String,
    #[serde(default = "default_registry_url")]
    pub registry_url: String,
    #[serde(default)]
    pub auth_token_path: Option<String>,
}

fn default_active_profile() -> String {
    "default".into()
}

fn default_registry_url() -> String {
    "https://raw.githubusercontent.com/PackWeave/registry/main/index.json".into()
}

impl Default for Config {
    fn default() -> Self {
        Self {
            active_profile: default_active_profile(),
            registry_url: default_registry_url(),
            auth_token_path: None,
        }
    }
}

impl Config {
    /// Path to the config file: `~/.packweave/config.toml`
    pub fn path() -> Result<PathBuf> {
        Ok(util::packweave_dir()?.join("config.toml"))
    }

    /// Load config from disk, returning defaults if file doesn't exist.
    pub fn load() -> Result<Self> {
        let path = Self::path()?;
        if !path.exists() {
            return Ok(Self::default());
        }
        let content = util::read_file(&path)?;
        toml::from_str(&content).map_err(|e| crate::error::WeaveError::Toml {
            path,
            source: Box::new(e),
        })
    }

    /// Save config to disk.
    pub fn save(&self) -> Result<()> {
        let path = Self::path()?;
        // Config only contains String/Option<String> fields — TOML serialization cannot fail.
        let content = toml::to_string_pretty(self).expect("Config serialization cannot fail");
        util::write_file(&path, &content)
    }
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
    }

    #[test]
    fn roundtrip_toml() {
        let config = Config::default();
        let toml_str = toml::to_string_pretty(&config).expect("serialize");
        let parsed: Config = toml::from_str(&toml_str).expect("deserialize");
        assert_eq!(parsed.active_profile, config.active_profile);
        assert_eq!(parsed.registry_url, config.registry_url);
    }
}
