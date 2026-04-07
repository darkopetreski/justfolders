use super::parser::CliParams;
use chrono::Local;
use std::fs::{self};
use std::path::PathBuf;
use thiserror::Error;

/// Errors from backup workflow steps such as destination validation.
#[derive(Debug, Error)]
pub enum BackupError {
    // #[error("missing destination path")]
    // MissingDest,
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
        let marker_path = self.params.dest_path().join("backup.marker");
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
        if !self.get_in_progress_folder().exists() {
            return Err(BackupError::MissingInProgressFolder);
        }

        let timestamp_folder = self.get_new_timestamp_folder();
        // Move tmp to a new folder with a current timestamp
        fs::rename(&self.get_in_progress_folder(), &timestamp_folder)?;

        // Remove latest symlink if exists, Create new sym link
        let latest_folder = self.get_latest_folder();
        if latest_folder.exists() || latest_folder.is_symlink() {
            fs::remove_file(&latest_folder)?;
        }

        #[cfg(unix)]
        {
            std::os::unix::fs::symlink(&timestamp_folder, &latest_folder)?;
        }
        #[cfg(windows)]
        {
            std::os::windows::fs::symlink_dir(&timestamp_folder, &latest_folder)?;
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

    /// Returns the path to the “latest” folder inside the destination directory.
    ///
    /// This combines the base destination path (from `dest_path()`) with the
    /// fixed folder name `"latest"`, typically used to reference the most recent backup
    /// or output.
    ///
    /// # Returns
    /// A `PathBuf` representing the latest folder path.
    pub fn get_latest_folder(&self) -> PathBuf {
        self.params.dest_path().join("latest")
    }

    /// Returns the path to the “in-progress” folder inside the destination directory.
    ///
    /// This combines the base destination path (from `dest_path()`) with the
    /// folder name returned by `in_progress_folder_name()`.
    ///
    /// # Returns
    /// A `PathBuf` representing the in-progress folder path.
    pub fn get_in_progress_folder(&self) -> PathBuf {
        self.params.dest_path().join(self.in_progress_folder_name())
    }

    /// Returns a new folder path inside the destination directory, named with the current timestamp.
    ///
    /// This combines the base destination path (from `dest_path()`) with a timestamp
    /// string generated by `now_string()`.
    ///
    /// # Returns
    /// A `PathBuf` representing the new timestamped folder.
    pub fn get_new_timestamp_folder(&self) -> PathBuf {
        self.params.dest_path().join(self.now_string())
    }

    /// Returns the full path to the log file if logging is enabled.
    ///
    /// The log file location is determined in the following order:
    /// 1. If `disable_logs` is set, returns `None`.
    /// 2. If a custom `log_dir` is provided, the log file is created there.
    /// 3. Otherwise, if a destination path is available, the log file is placed
    ///    in a `.logs` subdirectory inside it.
    ///
    /// The file name is generated using the current timestamp via `now_string()`.
    ///
    /// # Returns
    /// - `Some(PathBuf)` containing the full log file path
    /// - `None` if logging is disabled or no suitable directory is available
    pub fn get_log_filename(&self) -> Option<PathBuf> {
        if self.params.disable_logs {
            return None;
        }

        if let Some(log_dir) = self.params.log_dir.clone() {
            return Some(log_dir.join(self.now_string()));
        }

        Some(
            self.params
                .dest_path()
                .join(".logs")
                .join(self.now_string()),
        )
    }

    pub fn ensure_latest_points_correct_folder(&self) {
        //TODO: also case when latest is deleted, but there are backup folders

        let latest_link = self.get_latest_folder();
        if self.is_symlink_broken(&latest_link) {
            println!("broken symlink -------------------------------");
            if let Some(newest_backup) = self.newest_folder(&self.params.dest_path()) {
                println!("found folder {} and linking", newest_backup);

                fs::remove_file(&latest_link).unwrap();
                std::os::unix::fs::symlink(&newest_backup, &latest_link).unwrap();
            }
        }
    }

    fn is_symlink_broken(&self, path: &PathBuf) -> bool {
        // First, check if path exists as a symlink
        match fs::symlink_metadata(path) {
            Ok(meta) => {
                if meta.file_type().is_symlink() {
                    // Try to follow the symlink
                    fs::metadata(path).is_err()
                } else {
                    false // Not a symlink, so "not broken"
                }
            }
            Err(_) => false, // Path itself doesn't exist
        }
    }

    fn newest_folder(&self, dir: &PathBuf) -> Option<String> {
        let mut folders: Vec<String> = fs::read_dir(dir)
            .ok()?
            .filter_map(|entry| {
                let entry = entry.ok()?;
                let file_type = entry.file_type().ok()?;
                if file_type.is_dir() {
                    Some(entry.file_name().into_string().ok()?)
                } else {
                    None
                }
            })
            .collect();

        // Sort lexicographically
        folders.sort();

        // Last one is newest
        folders.pop()
    }
}
