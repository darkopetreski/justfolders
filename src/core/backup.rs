use super::parser::CliParams;
use chrono::Local;
use std::fs;
use std::path::PathBuf;
use thiserror::Error;

/// Errors from backup workflow steps such as destination validation.
#[derive(Debug, Error)]
pub enum BackupError {
    #[error("missing destination path")]
    MissingDest,

    #[error("missing backup.marker; create it with: touch {}", marker_path.display())]
    MissingMarker { marker_path: PathBuf },

    #[error("in-progress folder missing at path {:?}", path)]
    MissingInProgressFolder { path: PathBuf },

    #[error(transparent)]
    Io(#[from] std::io::Error),
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

    /// Finalizes the in-progress backup by moving it to a timestamped folder
    /// and updating the `latest` symlink.
    ///
    /// Steps:
    /// 1. Move the temporary folder (`tmp`) to `dest/YYYY-MM-DD-HHMMSS`.
    /// 2. Remove the existing `latest` symlink if it exists.
    /// 3. Create a new symlink `latest -> YYYY-MM-DD-HHMMSS`.
    ///
    /// # Errors
    /// Returns a `BackupError` if:
    /// - The destination path is missing (`MissingDest`).
    /// - The temporary in-progress folder does not exist (`MissingMarker`).
    /// - Any filesystem operation (move, symlink, remove) fails (`IoError`).
    ///
    pub fn move_in_progress_to_latest(&self) -> Result<(), BackupError> {
        let dest = self.params.dest.as_ref().ok_or(BackupError::MissingDest)?;
        let dest_path: PathBuf = dest.clone().into();

        // tmp folder
        let tmp_folder = dest_path.join(self.params.in_progress_folder_name());
        if !tmp_folder.exists() {
            return Err(BackupError::MissingInProgressFolder { path: tmp_folder });
        }

        // timestamped folder
        let timestamp_folder = dest_path.join(self.now_string());

        // Move tmp -> timestamped folder
        fs::rename(&tmp_folder, &timestamp_folder)?;

        // Update latest symlink
        let latest_link = dest_path.join("latest");

        // Remove old latest symlink if it exists
        if latest_link.exists() || latest_link.is_symlink() {
            fs::remove_file(&latest_link)?;
        }

        // Create new symlink: latest -> timestamp_folder
        std::os::unix::fs::symlink(&timestamp_folder, &latest_link)?;

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
