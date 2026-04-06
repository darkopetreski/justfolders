use std::path::PathBuf;

use chrono::Local;
use thiserror::Error;

use super::parser::CliParams;

/// Errors from backup workflow steps such as destination validation.
#[derive(Debug, Error)]
pub enum BackupError {
    #[error("missing destination path")]
    MissingDest,

    #[error("missing backup.marker; create it with: touch {}", marker_path.display())]
    MissingMarker { marker_path: PathBuf },
}

/// Borrows parsed CLI configuration and carries out the backup workflow (marker
/// checks, snapshot naming, rsync, logging) for a single CLI invocation.
///
/// The caller keeps owning [`CliParams`]; construct with [`Backup::new`] and keep
/// the args alive for as long as this value exists.
#[derive(Debug)]
pub struct Backup<'a> {
    /// Effective CLI arguments for this run (paths, rsync flags, logging, etc.).
    params: &'a CliParams,
}

impl<'a> Backup<'a> {
    /// Builds backup workflow state that references parsed CLI arguments.
    pub fn new(params: &'a CliParams) -> Self {
        Self { params }
    }

    /// Ensures that a backup marker file exists in the destination directory.
    ///
    /// This function performs the following steps:
    /// 1. Checks that the destination path (`self.params.dest`) is present.
    ///    - If not, returns `BackupError::MissingDest`.
    /// 2. Constructs the expected marker file path as `dest/backup.marker`.
    /// 3. Checks if the marker file exists on the filesystem.
    ///    - If the marker file is missing, returns `BackupError::MissingMarker` with the missing path.
    /// 4. If the marker file exists, returns `Ok(())`.
    ///
    /// # Errors
    /// - `BackupError::MissingDest` if the destination path is not set.
    /// - `BackupError::MissingMarker { marker_path }` if the marker file does not exist.
    /// 
    pub fn ensure_marker(&self) -> Result<(), BackupError> {
        let dest = self.params.dest.as_ref().ok_or(BackupError::MissingDest)?;
        let marker_path = PathBuf::from(dest).join("backup.marker");
        if !marker_path.exists() {
            return Err(BackupError::MissingMarker { marker_path });
        }
        Ok(())
    }

    /// Returns the current local date and time formatted as a compact string.
    ///
    /// The format is: `YYYY-MM-DD-HHMMSS`
    ///
    /// This is typically used for generating timestamped folder or file names
    /// (e.g. backup snapshots) that are:
    /// - human-readable
    /// - lexicographically sortable
    /// - filesystem-friendly (no spaces or special characters)
    /// 
    fn now_string(&self) -> String {
        Local::now().format("%Y-%m-%d-%H%M%S").to_string()
    }
}
