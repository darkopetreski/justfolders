use super::parser::CliParams;
use chrono::Local;
use std::fs::{self};
use std::path::PathBuf;
use thiserror::Error;

/// Errors from backup workflow steps such as destination validation.
#[derive(Debug, Error)]
pub enum BackupError {
    #[error("missing destination path")]
    MissingDest,

    #[error("missing backup.marker; create it with: touch {}", marker_path.display())]
    MissingMarker { marker_path: PathBuf },

    #[error("in-progress folder missing at path")]
    MissingInProgressFolder,

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
        // Get the in progress folder
        let in_progress_folder = self
            .get_in_progress_folder()
            .ok_or(BackupError::MissingDest)?;
        if !in_progress_folder.exists() {
            return Err(BackupError::MissingInProgressFolder);
        }

        // Move tmp to a new folder with a current timestamp
        let timestamp_folder = self
            .get_new_timestamp_folder()
            .ok_or(BackupError::MissingDest)?;
        fs::rename(&in_progress_folder, &timestamp_folder)?;

        // Remove lastst symlink if exists, Create new sym link
        let latest_folder = self.get_latest_folder().ok_or(BackupError::MissingDest)?;
        if latest_folder.exists() || latest_folder.is_symlink() {
            fs::remove_file(&latest_folder)?;
        }
        std::os::unix::fs::symlink(&timestamp_folder, &latest_folder)?;

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

    /// Returns the default name for the temporary “in-progress” folder used during backups.
    ///
    /// This folder is used internally to store files while a backup is in progress,
    /// typically under the destination directory. The default name is `"tmp"`.
    ///
    /// # Returns
    /// A `String` containing the folder name.
    ///
    pub fn in_progress_folder_name(&self) -> String {
        "tmp".to_string()
    }

    /// Returns the path to the `latest` subfolder inside the destination directory.
    ///
    /// This function checks whether `params.dest` is set:
    /// - If `Some`, it converts the destination into a [`PathBuf`] and appends `"latest"`
    ///   to construct the resulting path.
    /// - If `None`, it returns `None`.
    ///
    /// # Arguments
    ///
    /// * `params` - A reference to [`CliParams`] containing the optional destination path.
    ///
    /// # Returns
    ///
    /// * `Some(PathBuf)` containing `<dest>/latest` if a destination is provided.
    /// * `None` if `params.dest` is not set.
    ///
    pub fn get_latest_folder(&self) -> Option<PathBuf> {
        if let Some(d) = self.params.dest_path() {
            return Some(d.join("latest"));
        }

        None
    }

    /// Returns the path to the "in-progress" backup directory.
    ///
    /// This directory represents the temporary location where the current
    /// backup snapshot is being constructed before it is finalized.
    ///
    /// The path is derived by joining the destination directory with the
    /// internally generated in-progress folder name.
    ///
    /// # Returns
    /// - `Some(PathBuf)` if a destination path is available
    /// - `None` if `dest` is not set (e.g. when using `--rsync-get-flags`)
    ///
    /// # Notes
    /// - No validation is performed on the resulting path.
    /// - The returned path may not exist yet; callers are responsible for
    ///   creating or managing the directory lifecycle.
    pub fn get_in_progress_folder(&self) -> Option<PathBuf> {
        if let Some(d) = self.params.dest_path() {
            return Some(d.join(self.in_progress_folder_name()));
        }

        None
    }

    /// Returns the path for a new timestamped backup snapshot directory.
    ///
    /// This directory represents the final location of a completed backup
    /// snapshot, named using the current timestamp (via `now_string()`).
    ///
    /// The path is constructed by joining the destination directory with
    /// the generated timestamp string.
    ///
    /// # Returns
    /// - `Some(PathBuf)` if a destination path is available
    /// - `None` if `dest` is not set (e.g. when using `--rsync-get-flags`)
    ///
    /// # Notes
    /// - No validation is performed on the resulting path.
    /// - The returned path may not exist yet; callers are responsible for
    ///   creating the directory and ensuring timestamp uniqueness.
    /// - The format and resolution of the timestamp depend on `now_string()`.
    pub fn get_new_timestamp_folder(&self) -> Option<PathBuf> {
        if let Some(d) = self.params.dest_path() {
            return Some(d.join(self.now_string()));
        }

        None
    }

    /// Returns the full path to the log file for the current backup run.
    ///
    /// The file name is based on the current timestamp (via `now_string()`),
    /// ensuring uniqueness and chronological ordering.
    ///
    /// Resolution order:
    /// 1. If logging is disabled (`disable_logs` is `true`), returns `None`.
    /// 2. If `log_dir` is explicitly set, the log file will be created inside it.
    /// 3. Otherwise, if a destination path is available, the log file will be placed
    ///    under `<dest>/.logs/`.
    /// 4. If neither is available, returns `None`.
    ///
    /// # Returns
    /// - `Some(PathBuf)` containing the full path to the log file if logging is enabled.
    /// - `None` if logging is disabled or no suitable directory is available.
    ///
    /// # Notes
    /// - No directories are created automatically; the caller must ensure they exist.
    /// - The timestamp format is defined by `now_string()` and determines file name uniqueness.
    pub fn get_log_filename_ensure_dir_exists(&self) -> Option<PathBuf> {
        if self.params.disable_logs {
            return None;
        }

        if let Some(log_dir) = self.params.log_dir.clone() {
            if fs::create_dir_all(&log_dir).is_err() {
                return None;
            }
            return Some(log_dir.join(self.now_string()));
        }

        if let Some(dest_path) = self.params.dest_path() {
            let log_dir = dest_path.join(".logs");
            if fs::create_dir_all(&log_dir).is_err() {
                return None;
            }
            return Some(log_dir.join(self.now_string()));
        }

        None
    }
}
