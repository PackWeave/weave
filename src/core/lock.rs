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

    let file = File::create(&path).map_err(|e| WeaveError::io("creating lock file", e))?;

    file.try_lock_exclusive()
        .map_err(|_| WeaveError::LockContention { lock_path: path })?;

    Ok(WeaveFileLock { _file: file })
}

#[cfg(test)]
mod tests {
    use super::*;
    use serial_test::serial;
    use std::env;

    /// Helper: set `WEAVE_TEST_STORE_DIR` to a temp directory and return it.
    fn setup_test_store() -> tempfile::TempDir {
        let tmp = tempfile::tempdir().expect("create temp dir");
        // SAFETY: test helper, serial execution via #[serial]
        unsafe { env::set_var("WEAVE_TEST_STORE_DIR", tmp.path()) };
        tmp
    }

    fn teardown_test_store() {
        // SAFETY: test helper, serial execution via #[serial]
        unsafe { env::remove_var("WEAVE_TEST_STORE_DIR") };
    }

    #[test]
    #[serial]
    fn lock_file_is_created_in_store_directory() {
        let tmp = setup_test_store();
        let _lock = acquire().expect("acquire lock");
        assert!(tmp.path().join(".lock").exists());
        drop(_lock);
        teardown_test_store();
    }

    #[test]
    #[serial]
    fn second_lock_fails_with_contention() {
        let _tmp = setup_test_store();
        let _lock1 = acquire().expect("first acquire");
        let result = acquire();
        assert!(result.is_err());
        let err = result.unwrap_err();
        assert!(
            err.to_string().contains("another weave process is running"),
            "unexpected error message: {err}"
        );
        drop(_lock1);
        teardown_test_store();
    }

    #[test]
    #[serial]
    fn lock_released_on_drop() {
        let _tmp = setup_test_store();
        {
            let _lock = acquire().expect("acquire lock");
            // lock held here
        }
        // After drop, we should be able to acquire again.
        let _lock2 = acquire().expect("re-acquire after drop");
        drop(_lock2);
        teardown_test_store();
    }
}
