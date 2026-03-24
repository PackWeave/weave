use std::fs::File;
use std::path::PathBuf;

use fs2::FileExt;

use crate::error::{Result, WeaveError};
use crate::util;

/// An advisory file lock that prevents concurrent mutating weave operations.
///
/// The lock is held for the lifetime of this value and released automatically
/// when it is dropped (via [`fs2::FileExt::unlock`]).
#[derive(Debug)]
pub struct WeaveFileLock {
    _file: File,
}

impl Drop for WeaveFileLock {
    fn drop(&mut self) {
        // fs2 unlock is best-effort; the OS releases the lock when the
        // process exits regardless.
        let _ = self._file.unlock();
    }
}

/// Returns the path to the advisory lock file (`~/.packweave/.lock`).
fn lock_path() -> Result<PathBuf> {
    Ok(util::packweave_dir()?.join(".lock"))
}

/// Returns `true` if the I/O error indicates another process holds the lock.
///
/// On Unix this is `ErrorKind::WouldBlock`; on Windows the OS returns
/// `ERROR_LOCK_VIOLATION` (raw error 33) which the standard library does not
/// map to `WouldBlock`, so we check the raw code explicitly.
fn is_lock_contention(e: &std::io::Error) -> bool {
    if e.kind() == std::io::ErrorKind::WouldBlock {
        return true;
    }
    // Windows: ERROR_LOCK_VIOLATION = 33
    #[cfg(windows)]
    if e.raw_os_error() == Some(33) {
        return true;
    }
    false
}

/// Acquire an exclusive advisory lock for mutating weave operations.
///
/// Returns a [`WeaveFileLock`] guard that releases the lock on drop.
/// If another process already holds the lock, returns
/// [`WeaveError::LockContention`] immediately (non-blocking).
pub fn acquire() -> Result<WeaveFileLock> {
    let path = lock_path()?;

    // Ensure the parent directory exists.
    if let Some(parent) = path.parent() {
        util::ensure_dir(parent)?;
    }

    let file = std::fs::OpenOptions::new()
        .create(true)
        .truncate(false)
        .write(true)
        .open(&path)
        .map_err(|e| WeaveError::io("creating lock file", e))?;

    match file.try_lock_exclusive() {
        Ok(()) => Ok(WeaveFileLock { _file: file }),
        Err(e) if is_lock_contention(&e) => Err(WeaveError::LockContention { lock_path: path }),
        Err(e) => Err(WeaveError::io("acquiring lock file", e)),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serial_test::serial;

    /// RAII guard that sets an env var on creation and restores it on drop,
    /// even if the test panics.
    struct EnvGuard {
        key: &'static str,
        prev: Option<String>,
    }

    impl EnvGuard {
        fn set(key: &'static str, value: &std::path::Path) -> Self {
            let prev = std::env::var(key).ok();
            // SAFETY: test helper, serial execution via #[serial]
            unsafe { std::env::set_var(key, value) };
            Self { key, prev }
        }
    }

    impl Drop for EnvGuard {
        fn drop(&mut self) {
            // SAFETY: restoring env on drop in test
            match &self.prev {
                Some(v) => unsafe { std::env::set_var(self.key, v) },
                None => unsafe { std::env::remove_var(self.key) },
            }
        }
    }

    #[test]
    #[serial]
    fn lock_file_is_created_in_store_directory() {
        let tmp = tempfile::tempdir().expect("create temp dir");
        let _guard = EnvGuard::set("WEAVE_TEST_STORE_DIR", tmp.path());
        let _lock = acquire().expect("acquire lock");
        assert!(tmp.path().join(".lock").exists());
    }

    #[test]
    #[serial]
    fn second_lock_fails_with_contention() {
        let tmp = tempfile::tempdir().expect("create temp dir");
        let _guard = EnvGuard::set("WEAVE_TEST_STORE_DIR", tmp.path());
        let _lock1 = acquire().expect("first acquire");
        let result = acquire();
        assert!(result.is_err());
        let err = result.unwrap_err();
        assert!(
            err.to_string().contains("another weave process is running"),
            "unexpected error message: {err}"
        );
        assert!(
            err.to_string().contains("wait a moment and retry"),
            "unexpected error message: {err}"
        );
    }

    #[test]
    #[serial]
    fn lock_released_on_drop() {
        let tmp = tempfile::tempdir().expect("create temp dir");
        let _guard = EnvGuard::set("WEAVE_TEST_STORE_DIR", tmp.path());
        {
            let _lock = acquire().expect("acquire lock");
            // lock held here
        }
        // After drop, we should be able to acquire again.
        let _lock2 = acquire().expect("re-acquire after drop");
    }
}
