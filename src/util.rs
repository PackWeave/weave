use std::path::PathBuf;

use crate::error::{Result, WeaveError};

/// Returns the user's home directory or an error with an actionable message.
pub fn home_dir() -> Result<PathBuf> {
    dirs::home_dir().ok_or(WeaveError::NoHomeDir)
}

/// Returns the root directory for all weave state: `~/.packweave/`.
pub fn packweave_dir() -> Result<PathBuf> {
    Ok(home_dir()?.join(".packweave"))
}

/// Ensures a directory exists, creating it and all parents if necessary.
/// Returns an error if the path already exists as a non-directory (e.g. a file).
pub fn ensure_dir(path: &std::path::Path) -> Result<()> {
    if path.exists() {
        if !path.is_dir() {
            return Err(WeaveError::io(
                format!("creating directory {}", path.display()),
                std::io::Error::new(
                    std::io::ErrorKind::AlreadyExists,
                    "path already exists and is not a directory",
                ),
            ));
        }
        return Ok(());
    }
    std::fs::create_dir_all(path)
        .map_err(|e| WeaveError::io(format!("creating directory {}", path.display()), e))
}

/// Reads a file to string, returning a contextual error on failure.
pub fn read_file(path: &std::path::Path) -> Result<String> {
    std::fs::read_to_string(path)
        .map_err(|e| WeaveError::io(format!("reading {}", path.display()), e))
}

/// Writes content to a file, creating parent directories if needed.
pub fn write_file(path: &std::path::Path, content: &str) -> Result<()> {
    if let Some(parent) = path.parent() {
        ensure_dir(parent)?;
    }
    std::fs::write(path, content)
        .map_err(|e| WeaveError::io(format!("writing {}", path.display()), e))
}

/// Removes a file if it exists. No error if already absent.
pub fn remove_file_if_exists(path: &std::path::Path) -> Result<()> {
    match std::fs::remove_file(path) {
        Ok(()) => Ok(()),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(e) => Err(WeaveError::io(format!("removing {}", path.display()), e)),
    }
}
