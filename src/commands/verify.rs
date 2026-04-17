use crate::core::config::{Config, ConfigError};
use crate::core::manifest::VerifyFailure;
use crate::core::parser::VerifyArgs;
use crate::core::storage_backend::{from_config, StorageBackendError};
use thiserror::Error;

/// Expected snapshot name pattern: `YYYY-MM-DD-HHMMSS` (17 chars).
fn validate_snapshot_arg(name: &str) -> Result<(), VerifyError> {
    let valid = name.len() == 17
        && name.chars().enumerate().all(|(i, c)| match i {
            4 | 7 | 10 => c == '-',
            _ => c.is_ascii_digit(),
        });
    if valid {
        Ok(())
    } else {
        Err(VerifyError::InvalidSnapshotArg(name.to_string()))
    }
}

/// Errors that can occur during snapshot verification.
#[derive(Debug, Error)]
pub enum VerifyError {
    #[error(transparent)]
    Config(#[from] ConfigError),

    #[error(transparent)]
    Backend(#[from] StorageBackendError),

    #[error("no snapshots found at the destination")]
    NoSnapshots,

    /// Returned when verification completes but the snapshot is corrupt.
    /// The detailed failure list has already been printed to stderr before this
    /// error is returned, so main.rs will only show the one-line summary.
    #[error("snapshot '{0}' is corrupt ({1} failure(s) found)")]
    SnapshotCorrupt(String, usize),

    /// Returned when the `--snapshot` argument is not in `YYYY-MM-DD-HHMMSS` format.
    #[error("invalid --snapshot value '{0}': expected YYYY-MM-DD-HHMMSS format")]
    InvalidSnapshotArg(String),
}

/// Verifies a snapshot's integrity and reports any corrupt or missing files.
///
/// Exits with an error (and thus a non-zero exit code) if the snapshot is corrupt,
/// making it safe to use in scripts and monitoring pipelines.
pub fn run(args: VerifyArgs) -> Result<(), VerifyError> {
    let config = Config::load(&args.config)?;
    let backend = from_config(&config);

    // Resolve "latest" to the actual most-recent snapshot name so the output
    // and error messages always show the concrete snapshot that was checked.
    let snapshot_name = if args.snapshot == "latest" {
        backend
            .list_snapshots()?
            .into_iter()
            .last()
            .ok_or(VerifyError::NoSnapshots)?
    } else {
        // Validate the user-supplied name before passing it into the backend.
        validate_snapshot_arg(&args.snapshot)?;
        args.snapshot.clone()
    };

    println!("verifying snapshot {snapshot_name}...");

    let report = backend.verify_snapshot(&snapshot_name)?;

    if report.is_clean() {
        println!("{} files OK", report.ok_count);
        println!("snapshot is clean.");
        return Ok(());
    }

    // Print each failure before returning the error so the user has full detail.
    for failure in &report.failures {
        match failure {
            VerifyFailure::ChecksumMismatch { path, .. } => {
                eprintln!("  FAIL     {path}  (checksum mismatch)");
            }
            VerifyFailure::Missing { path } => {
                eprintln!("  MISSING  {path}");
            }
        }
    }

    let n = report.failures.len();
    eprintln!();
    eprintln!(
        "{} files OK, {n} problem(s) found",
        report.ok_count
    );
    eprintln!("snapshot is CORRUPT. Consider restoring from an earlier snapshot.");

    // List up to 5 other available snapshots as fallback options.
    if let Ok(mut all) = backend.list_snapshots() {
        all.retain(|s| s != &snapshot_name);
        if !all.is_empty() {
            eprintln!("\nOther available snapshots (newest first):");
            for s in all.iter().rev().take(5) {
                eprintln!("  {s}");
            }
        }
    }

    Err(VerifyError::SnapshotCorrupt(snapshot_name, n))
}
