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

    #[error("pack '{name}' is already installed (version {version})")]
    AlreadyInstalled { name: String, version: String },

    #[error("pack '{name}' is not installed")]
    NotInstalled { name: String },

    // Dependency errors
    #[error("dependency conflict: {0}")]
    DependencyConflict(String),

    // Store errors
    #[error("SHA256 checksum mismatch for '{name}' — expected {expected}, got {actual}. The archive may be corrupted; try installing again.")]
    ChecksumMismatch {
        name: String,
        expected: String,
        actual: String,
    },

    #[error("failed to download pack '{name}': {reason}")]
    DownloadFailed { name: String, reason: String },

    // Registry errors
    #[error("registry error: {0}")]
    Registry(String),

    // Adapter errors
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
        source: toml::de::Error,
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
