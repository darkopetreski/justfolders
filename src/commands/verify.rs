//! `justfolders verify` — re-hashes every file recorded in a snapshot's
//! `sha256sum` file and reports any drift.
//!
//! The implementation is intentionally thin: it loads the config, picks the
//! right backend, and asks the backend to run the stock GNU coreutils
//! `sha256sum -c sha256sum` binary inside the snapshot directory.  For a
//! local destination that's a direct subprocess; for a remote destination
//! it's the same command wrapped in `ssh …`.  Per-file `OK` / `FAILED`
//! lines stream straight to the user's terminal — this command is a
//! convenience wrapper, not a re-implementation.
//!
//! Exit codes:
//! * `0` — every file matched its recorded hash.
//! * `1` — one or more files mismatched, the snapshot has no `sha256sum`
//!   file, or a real I/O / SSH error occurred.

use crate::core::config::Config;
use crate::core::parser::VerifyArgs;
use crate::core::storage_backend::{from_config, StorageBackendError, VerifyOutcome};
use thiserror::Error;

/// Errors raised by the `verify` command.
///
/// The variants exist to give the user a clear, actionable message for each
/// failure mode rather than a single opaque "verify failed" line.
#[derive(Debug, Error)]
pub enum VerifyError {
    #[error(transparent)]
    Config(#[from] crate::core::config::ConfigError),

    #[error(transparent)]
    Backend(#[from] StorageBackendError),

    /// `sha256sum -c` reported at least one file whose contents no longer
    /// match the recorded hash.  The offending `FAILED` lines have already
    /// been printed to stderr by `sha256sum` itself.
    #[error("snapshot '{0}' has files that do not match their recorded hashes")]
    Mismatch(String),

    /// The snapshot directory exists but does not contain a `sha256sum`
    /// file — it predates the checksum feature, or was created when
    /// checksumming was disabled in the config.
    #[error(
        "snapshot '{0}' has no sha256sum file (was it created before the checksum feature, or with checksum = false?)"
    )]
    NoChecksumFile(String),
}

/// Loads the config, picks the matching backend, and runs the verification.
///
/// `args.snapshot` defaults to `"latest"` (set via clap), so the bare
/// invocation `justfolders verify --config foo.ini` checks the most recent
/// snapshot.  An explicit timestamp may be passed positionally.
pub fn run(args: VerifyArgs) -> Result<(), VerifyError> {
    let config = Config::load(&args.config)?;
    let backend = from_config(&config);

    println!("verifying snapshot '{}'…", args.snapshot);

    match backend.verify_snapshot(&args.snapshot)? {
        VerifyOutcome::Clean => {
            println!("snapshot '{}' verified OK", args.snapshot);
            Ok(())
        }
        VerifyOutcome::Mismatch => Err(VerifyError::Mismatch(args.snapshot)),
        VerifyOutcome::NoChecksumFile => Err(VerifyError::NoChecksumFile(args.snapshot)),
    }
}
