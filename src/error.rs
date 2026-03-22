use std::path::PathBuf;

use thiserror::Error;

#[derive(Debug, Error)]
pub enum WeaveError {
    // Pack errors
    #[error("pack '{name}' not found in registry")]
    PackNotFound { name: String },

    #[error("pack '{name}' version {version} not found — available versions: {available}")]
    VersionNotFound {
        name: String,
        version: String,
        available: String,
    },

    #[error("invalid pack manifest at {path}: {reason}")]
    InvalidManifest { path: PathBuf, reason: String },

    #[allow(dead_code)]
    #[error("pack '{name}' is already installed (version {version})")]
    AlreadyInstalled { name: String, version: String },

    #[error("pack '{name}' is not installed — run `weave list` to see installed packs")]
    NotInstalled { name: String },

    // Profile errors
    #[error("profile '{name}' not found — run `weave profile list` to see available profiles")]
    ProfileNotFound { name: String },

    #[error("cannot delete the active profile '{name}' — switch to another profile first with `weave use <profile>`")]
    ActiveProfileDeletion { name: String },

    #[error("cannot delete the 'default' profile — it is required")]
    DefaultProfileDeletion,

    #[error("invalid profile name '{name}' — only letters, digits, hyphens, and underscores are allowed")]
    InvalidProfileName { name: String },

    // Registry errors
    #[error("pack '{name}' has no releases in the registry")]
    NoReleases { name: String },

    // Dependency errors
    #[error("circular dependency detected involving '{pack}' (chain: {chain})")]
    CircularDependency { pack: String, chain: String },

    #[error("dependency conflict for '{pack}': {conflicts}")]
    DependencyConflict { pack: String, conflicts: String },

    // Store errors
    #[error("SHA256 checksum mismatch for '{name}' — expected {expected}, got {actual}. The archive may be corrupted; try installing again.")]
    ChecksumMismatch {
        name: String,
        expected: String,
        actual: String,
    },

    #[error("failed to download pack '{name}': {reason}")]
    DownloadFailed { name: String, reason: String },

    #[error("registry error: {0}")]
    Registry(String),

    #[error("registry HTTP {status} for {url}")]
    RegistryHttp { status: u16, url: String },

    #[error("MCP Registry error: {0}")]
    McpRegistry(String),

    // Adapter errors
    #[allow(dead_code)]
    #[error("{cli} is not installed on this system")]
    CliNotInstalled { cli: String },

    #[error("failed to apply pack '{pack}' to {cli}: {reason}")]
    ApplyFailed {
        pack: String,
        cli: String,
        reason: String,
    },

    #[error("failed to remove pack '{pack}' from {cli}: {reason}")]
    RemoveFailed {
        pack: String,
        cli: String,
        reason: String,
    },

    // Config errors
    #[error("could not determine home directory — set the HOME environment variable")]
    NoHomeDir,

    // IO errors
    #[error("{context}: {source}")]
    Io {
        context: String,
        source: std::io::Error,
    },

    // JSON errors
    #[error("failed to parse JSON at {path}: {source}")]
    Json {
        path: PathBuf,
        source: serde_json::Error,
    },

    // TOML errors
    #[error("failed to parse TOML at {path}: {source}")]
    Toml {
        path: PathBuf,
        source: Box<toml::de::Error>,
    },
}

impl WeaveError {
    pub fn io(context: impl Into<String>, source: std::io::Error) -> Self {
        Self::Io {
            context: context.into(),
            source,
        }
    }
}

pub type Result<T> = std::result::Result<T, WeaveError>;
