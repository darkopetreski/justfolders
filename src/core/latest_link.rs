use crate::core::backup::Backup;

use std::fs;
use std::path::{Path, PathBuf};
use thiserror::Error;

/// Errors that can occur when managing backup symbolic links.
#[derive(Debug, Error)]
pub enum BackupLinkError {
    /// No valid backup folders were found in the backup destination.
    #[error("No backup folders found in {0}")]
    NoBackups(PathBuf),

    /// The path exists but is not a symlink.
    // #[error("Path exists but is not a symlink: {0}")]
    // NotASymlink(PathBuf),

    /// Underlying I/O error.
    #[error(transparent)]
    Io(#[from] std::io::Error),
}

/// Ensures that the `latest` symbolic link points to the most recent backup folder,
/// creating or repairing it as needed. This is a recovery if the user has messed up the latest symlink.
///
/// This function performs the following actions:
/// 1. If the symlink does not exist and backup folders exist, it creates the symlink
///    pointing to the latest backup.
/// 2. If the symlink exists but is broken (points to a missing target), it updates
///    the symlink to point to the latest backup.
/// 3. If the path exists but is **not a symlink** (e.g., a file or directory),
///    it removes the existing path and creates a proper symlink to the latest backup.
///
/// # Arguments
///
/// * `backup` - A reference to a `Backup` object providing the backup destination
///              and the path of the latest folder symlink.
///
/// # Errors
///
/// Returns a `BackupLinkError` in the following cases:
/// - No valid backup folders are found (`BackupLinkError::NoBackups`).
/// - Underlying filesystem operations fail (`BackupLinkError::Io`), e.g., reading directories,
///   removing files or directories, or creating the symlink.
pub fn ensure_latest_backup_link(backup: &Backup) -> Result<(), BackupLinkError> {
    let base_dir = backup.get_params().dest_path();
    let link_path = backup.get_latest_folder();

    let latest = find_latest_backup_folder(&base_dir)?
        .ok_or_else(|| BackupLinkError::NoBackups(base_dir.clone()))?;

    // Try to get metadata of the symlink itself
    let metadata = fs::symlink_metadata(&link_path).ok(); // returns Option<Metadata>

    match metadata {
        Some(metadata) if metadata.file_type().is_symlink() => {
            // Symlink exists (maybe broken)
            let target = fs::read_link(&link_path)?;
            if !target.exists() {
                // Broken symlink → fix it
                fs::remove_file(&link_path)?;
                create_symlink(&latest, &link_path)?;
            }
        }
        Some(metadata) => {
            // Path exists but is not a symlink → remove and recreate
            if metadata.is_dir() {
                fs::remove_dir_all(&link_path)?;
            } else {
                fs::remove_file(&link_path)?;
            }
            create_symlink(&latest, &link_path)?;
        }
        None => {
            // Path does not exist → create symlink
            create_symlink(&latest, &link_path)?;
        }
    }

    Ok(())
}

/// Finds the latest valid backup folder in the given base directory.
///
/// Scans all directories in `base_dir`, filters out invalid backup folder names,
/// sorts them alphabetically, and returns the path to the newest folder.
///
/// # Arguments
///
/// * `base_dir` - Path to the directory containing backup folders.
///
/// # Returns
///
/// Returns `Ok(Some(PathBuf))` with the path to the latest backup folder if any valid
/// backups exist, or `Ok(None)` if no valid backup folders are found.
///
/// # Errors
///
/// Returns a `BackupLinkError::Io` if reading the directory contents fails.
fn find_latest_backup_folder(base_dir: &PathBuf) -> Result<Option<PathBuf>, BackupLinkError> {
    let mut backups: Vec<(String, PathBuf)> = Vec::new();

    for entry in fs::read_dir(base_dir)? {
        let entry = entry?;
        let path = entry.path();

        if !path.is_dir() {
            continue;
        }

        let name = match path.file_name().and_then(|n| n.to_str()) {
            Some(n) => n.to_string(),
            None => continue,
        };

        if is_valid_backup_name(&name) {
            backups.push((name, path));
        }
    }

    // Sort alphabetically (oldest → newest)
    backups.sort_by(|a, b| a.0.cmp(&b.0));

    // Take the latest (last element)
    Ok(backups.pop().map(|(_, path)| path))
}

/// Checks whether a folder name is a valid backup folder name.
///
/// Valid backup folder names must follow the format `YYYY-MM-DD-HHMMSS`, where:
/// - `YYYY` is a 4-digit year
/// - `MM` is a 2-digit month
/// - `DD` is a 2-digit day
/// - `HHMMSS` is a 6-digit time in hours, minutes, and seconds
///
/// # Arguments
///
/// * `name` - The folder name to validate.
///
/// # Returns
///
/// `true` if the name matches the expected backup format, `false` otherwise.
fn is_valid_backup_name(name: &str) -> bool {
    let parts: Vec<&str> = name.split('-').collect();
    if parts.len() != 4 {
        return false;
    }

    parts[0].len() == 4
        && parts[0].chars().all(|c| c.is_ascii_digit())
        && parts[1].len() == 2
        && parts[1].chars().all(|c| c.is_ascii_digit())
        && parts[2].len() == 2
        && parts[2].chars().all(|c| c.is_ascii_digit())
        && parts[3].len() == 6
        && parts[3].chars().all(|c| c.is_ascii_digit())
}

/// Creates a symbolic link pointing to the target folder (Unix version).
#[cfg(unix)]
fn create_symlink(target: &Path, link: &Path) -> Result<(), BackupLinkError> {
    std::os::unix::fs::symlink(target, link)?;
    Ok(())
}

/// Creates a symbolic link pointing to the target folder (Windows version).
///
/// On Windows, only directory symlinks are supported by this function.
#[cfg(windows)]
fn create_symlink(target: &Path, link: &Path) -> Result<(), BackupLinkError> {
    std::os::windows::fs::symlink_dir(target, link)?;
    Ok(())
}
