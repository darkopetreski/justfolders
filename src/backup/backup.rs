use chrono::Local;
use std::path::PathBuf;

/// Ensures that the destination directory is a valid backup target.
///
/// This function checks for the presence of a `backup.marker` file inside
/// the given destination directory. The marker file acts as a safeguard to
/// prevent accidental operations on unintended directories (e.g. deleting
/// or overwriting the wrong path).
///
/// # Arguments
/// * `dest` - Path to the destination directory that should contain the marker file.
///
/// # Returns
/// * `Ok(())` if the marker file exists
/// * `Err(String)` with a helpful message if the marker is missing
///
/// # Errors
/// Returns an error when `backup.marker` is not found in the destination.
/// The error message includes a suggested command to create it.
///
/// # Example
/// ```bash
/// touch /your/destination/backup.marker
/// ```
pub fn ensure_marker(dest: &PathBuf) -> Result<(), String> {
    let marker = dest.join("backup.marker");
    if !marker.exists() {
        return Err(format!(
            "Missing backup.marker use 'touch {}'",
            marker.display()
        ));
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
fn now_string() -> String {
    Local::now().format("%Y-%m-%d-%H%M%S").to_string()
}
