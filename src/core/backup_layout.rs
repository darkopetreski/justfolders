use crate::core::config::Config;
use crate::core::storage_backend::{StorageBackend, StorageBackendError};
use chrono::Local;
use std::path::PathBuf;

const DEFAULT_LOG_DIR_NAME: &str = "justfolders";

const IN_PROGRESS_NAME: &str = "tmp";
const LATEST_NAME: &str = "latest";
const LOGS_DIR_NAME: &str = ".logs";

// ── BackupLayout ─────────────────────────────────────────────────────────────

/// Derives all folder and file paths needed during a backup run from a [`Config`].
///
/// **Rsync-facing** methods return `String` values that are valid rsync path
/// arguments for both local (`/dest/tmp`) and remote (`user@host:/dest/tmp`)
/// destinations.
///
/// Filesystem mutations (rename, symlink) are delegated to a [`StorageBackend`]
/// so the same logic works for local and remote destinations.
#[derive(Debug)]
pub struct BackupLayout<'a> {
    config: &'a Config,
    /// Timestamp captured once at construction so all paths in a single run
    /// share the same value.
    timestamp: String,
}

impl<'a> BackupLayout<'a> {
    pub fn new(config: &'a Config) -> Self {
        Self {
            config,
            timestamp: Local::now().format("%Y-%m-%d-%H%M%S").to_string(),
        }
    }

    // ── Rsync-facing (String) ────────────────────────────────────────────────

    /// Path rsync writes into while the backup is in progress.
    ///
    /// e.g. `/dest/tmp` or `user@host:/dest/tmp`
    pub fn in_progress_arg(&self) -> String {
        self.config.dest.join(IN_PROGRESS_NAME)
    }

    /// Path of the `latest` symlink — used as `--link-dest` so unchanged files
    /// are hard-linked rather than copied again.
    ///
    /// e.g. `/dest/latest` or `user@host:/dest/latest`
    pub fn latest_arg(&self) -> String {
        self.config.dest.join(LATEST_NAME)
    }

    /// Path of the timestamped snapshot folder that `tmp` will be renamed to
    /// after a successful rsync.
    ///
    /// e.g. `/dest/2025-04-14-120000` or `user@host:/dest/2025-04-14-120000`
    pub fn timestamp_arg(&self) -> String {
        self.config.dest.join(&self.timestamp)
    }

    /// The bare timestamp string for this run (e.g. `"2025-04-14-120000"`).
    pub fn timestamp(&self) -> &str {
        &self.timestamp
    }

    // ── Finalization ─────────────────────────────────────────────────────────

    /// Finalizes the backup by renaming `tmp` to a timestamped folder and
    /// updating the `latest` symlink, via the provided [`StorageBackend`].
    ///
    /// Works for both local and remote destinations.
    ///
    /// # Errors
    /// Propagates any [`StorageBackendError`] from the backend.
    pub fn move_in_progress_to_latest(
        &self,
        backend: &dyn StorageBackend,
    ) -> Result<(), StorageBackendError> {
        backend.rename_in_progress(&self.timestamp)?;
        backend.update_latest_symlink(&self.timestamp)?;
        Ok(())
    }

    // ── Logging ──────────────────────────────────────────────────────────────

    /// Returns the local path for the rsync `--log-file` argument, or `None`
    /// if logging is disabled.
    ///
    /// The log file is always written on the machine running rsync (local).
    /// Resolution order:
    /// 1. `disable_logs = true` → `None`
    /// 2. Explicit `log_dir` in config → `<log_dir>/<timestamp>`
    /// 3. Local dest → `<dest>/.logs/<timestamp>`
    /// 4. Remote dest → `<data_dir>/justfolders/<timestamp>`
    ///    where `<data_dir>` is the OS data directory (e.g. `~/.local/share` on Linux).
    ///    Falls back to `None` if the OS provides no data directory.
    pub fn log_path(&self) -> Option<PathBuf> {
        if self.config.disable_logs {
            return None;
        }

        if let Some(log_dir) = &self.config.log_dir {
            return Some(log_dir.join(&self.timestamp));
        }

        if let Some(local) = self.config.dest.local_path() {
            return Some(local.join(LOGS_DIR_NAME).join(&self.timestamp));
        }

        // Remote destination: fall back to the OS-provided local data directory.
        dirs::data_dir().map(|d| d.join(DEFAULT_LOG_DIR_NAME).join(&self.timestamp))
    }
}
