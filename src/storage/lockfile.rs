//! Process Lock File Module
//!
//! Prevents multiple instances of SAIREN-OS from running simultaneously,
//! which would cause database lock conflicts with sled.

use anyhow::{bail, Context, Result};
use std::fs::{self, File};
use std::io::{Read, Write};
use std::path::{Path, PathBuf};

/// Process lock file manager
///
/// Creates a lock file with the current process ID to prevent
/// multiple instances from accessing the database simultaneously.
#[derive(Debug)]
pub struct ProcessLock {
    lock_path: PathBuf,
    owned: bool,
}

impl ProcessLock {
    /// Lock file name
    const LOCK_FILE_NAME: &'static str = ".sairen.lock";

    /// Acquire a process lock for the given data directory
    ///
    /// Returns an error if another instance is already running.
    pub fn acquire<P: AsRef<Path>>(data_dir: P) -> Result<Self> {
        let data_dir = data_dir.as_ref();

        // Ensure data directory exists
        fs::create_dir_all(data_dir)
            .with_context(|| format!("Failed to create data directory: {:?}", data_dir))?;

        let lock_path = data_dir.join(Self::LOCK_FILE_NAME);

        // Check for existing lock
        if lock_path.exists() {
            match Self::check_existing_lock(&lock_path) {
                Ok(Some(pid)) => {
                    // Another process is holding the lock
                    bail!(
                        "Another SAIREN-OS instance is already running (PID: {})\n\
                         \n\
                         To resolve this:\n\
                         1. Stop the other instance, or\n\
                         2. If no other instance is running, remove the stale lock file:\n\
                            rm {:?}",
                        pid,
                        lock_path
                    );
                }
                Ok(None) => {
                    // Stale lock file, remove it
                    tracing::info!("Removing stale lock file from previous instance");
                    fs::remove_file(&lock_path)
                        .context("Failed to remove stale lock file")?;
                }
                Err(e) => {
                    tracing::warn!("Error checking existing lock: {}", e);
                    // Try to proceed anyway by removing the lock
                    let _ = fs::remove_file(&lock_path);
                }
            }
        }

        // Create new lock file with our PID
        let pid = std::process::id();
        let mut file = File::create(&lock_path)
            .with_context(|| format!("Failed to create lock file: {:?}", lock_path))?;

        writeln!(file, "{}", pid)
            .context("Failed to write PID to lock file")?;

        tracing::debug!("Acquired process lock (PID: {}) at {:?}", pid, lock_path);

        Ok(Self {
            lock_path,
            owned: true,
        })
    }

    /// Check if an existing lock file is held by a running process
    ///
    /// Returns:
    /// - `Ok(Some(pid))` if the lock is held by a running process
    /// - `Ok(None)` if the lock file exists but the process is not running (stale)
    /// - `Err(_)` if there was an error reading/parsing the lock file
    fn check_existing_lock(lock_path: &Path) -> Result<Option<u32>> {
        let mut file = File::open(lock_path)
            .context("Failed to open existing lock file")?;

        let mut contents = String::new();
        file.read_to_string(&mut contents)
            .context("Failed to read lock file contents")?;

        let pid: u32 = contents
            .trim()
            .parse()
            .context("Failed to parse PID from lock file")?;

        // Check if the process is still running
        if Self::is_process_running(pid) {
            Ok(Some(pid))
        } else {
            Ok(None)
        }
    }

    /// Check if a process with the given PID is still running
    #[cfg(unix)]
    fn is_process_running(pid: u32) -> bool {
        // On Unix, we can use kill with signal 0 to check if process exists
        // This doesn't actually send a signal, just checks if the process exists
        

        // Try to read /proc/PID/cmdline to verify it's a SAIREN process
        let proc_path = format!("/proc/{}/cmdline", pid);
        if let Ok(cmdline) = fs::read_to_string(&proc_path) {
            // Check if it looks like our process
            cmdline.contains("sairen") || cmdline.contains("tds-guardian")
        } else {
            // Process doesn't exist or we can't read it
            false
        }
    }

    #[cfg(not(unix))]
    fn is_process_running(_pid: u32) -> bool {
        // On non-Unix systems, assume the process might be running
        // This is a conservative approach
        true
    }

    /// Release the lock (called automatically on drop)
    pub fn release(&mut self) {
        if self.owned {
            if let Err(e) = fs::remove_file(&self.lock_path) {
                tracing::warn!("Failed to remove lock file: {}", e);
            } else {
                tracing::debug!("Released process lock at {:?}", self.lock_path);
            }
            self.owned = false;
        }
    }

    /// Get the path to the lock file
    #[cfg(test)]
    pub fn path(&self) -> &Path {
        &self.lock_path
    }
}

impl Drop for ProcessLock {
    fn drop(&mut self) {
        self.release();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn test_acquire_lock() {
        let temp_dir = tempdir().unwrap();
        let lock = ProcessLock::acquire(temp_dir.path()).unwrap();

        // Lock file should exist
        assert!(lock.path().exists());

        // Lock file should contain our PID
        let contents = fs::read_to_string(lock.path()).unwrap();
        let pid: u32 = contents.trim().parse().unwrap();
        assert_eq!(pid, std::process::id());
    }

    #[test]
    fn test_lock_released_on_drop() {
        let temp_dir = tempdir().unwrap();
        let lock_path;

        {
            let lock = ProcessLock::acquire(temp_dir.path()).unwrap();
            lock_path = lock.path().to_path_buf();
            assert!(lock_path.exists());
        }

        // Lock should be released after drop
        assert!(!lock_path.exists());
    }

    #[test]
    fn test_stale_lock_removed() {
        let temp_dir = tempdir().unwrap();
        let lock_path = temp_dir.path().join(ProcessLock::LOCK_FILE_NAME);

        // Create a stale lock with a non-existent PID
        fs::write(&lock_path, "999999999\n").unwrap();

        // Should be able to acquire lock (stale lock removed)
        let lock = ProcessLock::acquire(temp_dir.path()).unwrap();
        assert!(lock.path().exists());
    }
}
