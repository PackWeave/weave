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

    #[error("pack '{name}' is not installed — {hint}")]
    NotInstalled { name: String, hint: String },

    #[error(
        "pack '{pack_name}' requires weave {required} or later, but this is weave {current} — please upgrade"
    )]
    IncompatibleToolVersion {
        pack_name: String,
        required: semver::Version,
        current: semver::Version,
    },

    // Install/update validation errors
    #[error(
        "pack manifest {field} '{actual}' does not match resolved {field} '{expected}' — the archive may be corrupt or tampered"
    )]
    ManifestMismatch {
        field: &'static str,
        expected: String,
        actual: String,
    },

    #[error("invalid version requirement '{input}' — {reason}")]
    InvalidVersionReq { input: String, reason: String },

    #[error(
        "pack '{name}' is not available from {source_type} and is not in the local store — {hint}"
    )]
    PackNotAvailable {
        name: String,
        source_type: String,
        hint: String,
    },

    // Auth errors
    #[allow(dead_code)]
    #[error("not authenticated — run `weave auth login` to authenticate")]
    NotAuthenticated,

    // Publish errors
    #[error(
        "version {version} of pack '{name}' is already published — bump the version in pack.toml"
    )]
    VersionAlreadyPublished { name: String, version: String },

    #[error("failed to publish '{name}@{version}': {reason}")]
    PublishFailed {
        name: String,
        version: String,
        reason: String,
    },

    // Profile errors
    #[error("profile '{name}' not found — run `weave profile list` to see available profiles")]
    ProfileNotFound { name: String },

    #[error(
        "cannot delete the active profile '{name}' — switch to another profile first with `weave use <profile>`"
    )]
    ActiveProfileDeletion { name: String },

    #[error("cannot delete the 'default' profile — it is required")]
    DefaultProfileDeletion,

    #[error(
        "invalid profile name '{name}' — only letters, digits, hyphens, and underscores are allowed"
    )]
    InvalidProfileName { name: String },

    // Registry errors
    #[error("pack '{name}' has no releases in the registry")]
    NoReleases { name: String },

    // Dependency errors
    #[error("circular dependency detected involving '{pack}' (chain: {chain})")]
    CircularDependency { pack: String, chain: String },

    #[error("dependency conflict for '{pack}': {conflicts}")]
    DependencyConflict { pack: String, conflicts: String },

    #[error("registry error: {0}")]
    Registry(String),

    #[error(
        "registry HTTP {status} for {url} — check your registry_url / WEAVE_REGISTRY_URL setting, network connectivity, or whether the registry is available"
    )]
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

    // Tap errors
    #[error("invalid tap name '{name}' — expected format 'user/repo' (e.g. 'acme/my-packs')")]
    InvalidTapName { name: String },

    #[error("tap '{name}' is already registered — run `weave tap list` to see registered taps")]
    TapAlreadyExists { name: String },

    #[error("tap '{name}' is not registered — run `weave tap list` to see registered taps")]
    TapNotFound { name: String },

    // Concurrency errors
    #[error(
        "another weave process is running — wait a moment and retry, or remove {lock_path} if this is unexpected"
    )]
    LockContention { lock_path: PathBuf },

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

    // TOML edit errors (format-preserving writes)
    #[error("invalid TOML in {path}: {source}")]
    TomlEdit {
        path: PathBuf,
        source: toml_edit::TomlError,
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn manifest_mismatch_error_message() {
        let err = WeaveError::ManifestMismatch {
            field: "name",
            expected: "webdev".to_string(),
            actual: "other-pack".to_string(),
        };
        let msg = err.to_string();
        assert!(msg.contains("name"));
        assert!(msg.contains("'other-pack'"));
        assert!(msg.contains("'webdev'"));
        assert!(msg.contains("corrupt or tampered"));
    }

    #[test]
    fn invalid_version_req_error_message() {
        let err = WeaveError::InvalidVersionReq {
            input: "not-a-version".to_string(),
            reason: "unexpected character 'n'".to_string(),
        };
        let msg = err.to_string();
        assert!(msg.contains("'not-a-version'"));
        assert!(msg.contains("unexpected character 'n'"));
    }

    #[test]
    fn pack_not_available_error_message() {
        let err = WeaveError::PackNotAvailable {
            name: "webdev".to_string(),
            source_type: "local (/tmp/webdev)".to_string(),
            hint: "reinstall from the original local path with `weave install --local /tmp/webdev`"
                .to_string(),
        };
        let msg = err.to_string();
        assert!(msg.contains("'webdev'"));
        assert!(msg.contains("not available from local"));
        assert!(msg.contains("reinstall"));
    }
}
