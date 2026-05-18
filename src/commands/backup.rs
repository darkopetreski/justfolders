use crate::core::backup_layout::BackupLayout;
use crate::core::config::Config;
use crate::core::notify::{self, NotifyState};
use crate::core::parser::BackupArgs;
use crate::core::retention::{apply_policy, print_snapshot_groups};
use crate::core::rsync::{Rsync, RsyncError, RsyncOutcome};
use crate::core::storage_backend::{StorageBackend, StorageBackendError};
use thiserror::Error;

/// RAII guard that holds the `backup.lock` for the duration of a backup run.
///
/// The lock is released automatically when the guard is dropped, so it is
/// always cleaned up whether the run succeeds or returns early via `?`.
struct BackupLock<'a> {
    backend: &'a dyn StorageBackend,
}

impl<'a> BackupLock<'a> {
    /// Acquires the backup lock by calling [`StorageBackend::acquire_lock`].
    ///
    /// Returns the guard on success.  The lock is released automatically when
    /// the guard is dropped, even if the backup run returns early via `?`.
    fn acquire(backend: &'a dyn StorageBackend) -> Result<Self, StorageBackendError> {
        backend.acquire_lock()?;
        Ok(Self { backend })
    }
}

impl Drop for BackupLock<'_> {
    /// Releases the backup lock unconditionally.
    ///
    /// Errors are printed to stderr but never propagated — a failed unlock
    /// must not mask the original backup error or a successful result.
    fn drop(&mut self) {
        if let Err(e) = self.backend.release_lock() {
            eprintln!("warning: failed to release backup lock: {e}");
        }
    }
}

/// Errors that can occur during a backup run.
#[derive(Debug, Error)]
pub enum BackupError {
    #[error(transparent)]
    Config(#[from] crate::core::config::ConfigError),

    #[error(transparent)]
    Rsync(#[from] RsyncError),

    #[error(transparent)]
    Backend(#[from] StorageBackendError),

    /// rsync does not support two remote hosts in a single invocation.
    /// Supported transfer directions are:
    ///   - local  → local
    ///   - remote → local  (pull)
    ///   - local  → remote (push)
    #[error("both source and destination are remote; rsync requires at least one local side")]
    BothRemote,

    /// rsync exited with code 23 or 24 (some files were not transferred) and
    /// the user has set `strict = true`, so the partial transfer is treated
    /// as a hard failure: the snapshot is discarded and no rotation happens.
    #[error("rsync completed with a partial transfer (code {code}); strict mode is enabled so the snapshot was discarded\n{stderr}")]
    PartialTransfer { code: i32, stderr: String },
}

/// Outcome of a successful backup run.
///
/// Distinguishes a clean run from one that completed with warnings (some
/// files skipped) so the caller can pick the right notification.  A hard
/// failure is reported via [`BackupError`] instead.
enum BackupOutcome {
    Clean,
    Warnings { stderr: String },
}

/// Runs a full backup: loads the config, validates it, runs rsync, and finalizes the snapshot.
///
/// If `[notify]` is configured, an email is sent after every run:
/// - always on failure
/// - on warnings when `on_warnings = true` (default)
/// - on success when `on_success = true`
pub fn run(args: BackupArgs) -> Result<(), BackupError> {
    let config = Config::load(&args.config)?;

    if config.src.is_remote() && config.dest.is_remote() {
        return Err(BackupError::BothRemote);
    }

    let src = config.src.original.clone();
    let outcome = do_backup(&config);

    if let Some(ref smtp) = config.notify {
        // Render the error to an owned String first so it can be borrowed by
        // NotifyState::Failed for the duration of the send call.
        let err_detail = outcome.as_ref().err().map(|e| e.to_string());
        let state = match (&outcome, &err_detail) {
            (Ok(BackupOutcome::Clean), _) => NotifyState::Success,
            (Ok(BackupOutcome::Warnings { stderr }), _) => NotifyState::Warnings { stderr },
            (Err(_), Some(detail)) => NotifyState::Failed { detail },
            (Err(_), None) => unreachable!("err_detail is Some whenever outcome is Err"),
        };
        notify::send_backup_result(smtp, state, &src);
    }

    outcome.map(|_| ())
}

/// Performs the actual backup steps: acquire lock → rsync → finalize → apply policy → print summary.
///
/// Separated from [`run`] so the notification logic in `run` can wrap a single
/// `Result` regardless of which step failed.
///
/// Returns [`BackupOutcome::Warnings`] when rsync reported a partial transfer
/// (some files skipped) but [`Config::strict`] is `false` — in that case the
/// snapshot is still created.  With `strict = true` the partial transfer is
/// escalated to [`BackupError::PartialTransfer`] *before* the snapshot is
/// promoted to `latest`, so the `.in_progress` directory is left on disk for
/// inspection and `latest` is unchanged.
fn do_backup(config: &Config) -> Result<BackupOutcome, BackupError> {
    let layout = BackupLayout::new(config);
    layout.prepare_log_dir()?;

    // Acquire lock before any I/O — released automatically when `_lock` is dropped.
    let _lock = BackupLock::acquire(layout.backend())?;

    // Capture the rsync outcome so we can react to a partial transfer.  In
    // strict mode any skipped file is fatal and we abort before promoting the
    // in-progress directory to a real snapshot.
    let warnings = match Rsync::new(config, &layout)?.run()? {
        RsyncOutcome::Clean => None,
        RsyncOutcome::Warnings { code, stderr } => {
            if config.strict {
                return Err(BackupError::PartialTransfer { code, stderr });
            }
            Some(stderr)
        }
    };

    layout.move_in_progress_to_latest()?;

    print!("generating manifest... ");
    layout.backend().generate_manifest(layout.timestamp())?;
    println!("done");

    let snapshots = layout.backend().list_snapshots()?;
    let mut to_delete = apply_policy(&config.policy, &snapshots);

    // Belt-and-suspenders: the retention algorithm guarantees the just-created
    // snapshot is always claimed by some rule and never appears in to_delete.
    // This explicit guard makes that invariant immune to any future regression —
    // the snapshot we just created must never be deleted in the same run.
    to_delete.retain(|name| name != layout.timestamp());

    for name in &to_delete {
        layout.backend().delete_snapshot(name)?;
    }

    if warnings.is_some() {
        println!(
            "backup completed with warnings: {} → {}",
            config.src.original,
            layout.timestamp_arg()
        );
        println!("some files were skipped — see rsync output above");
    } else {
        println!(
            "backup complete: {} → {}",
            config.src.original,
            layout.timestamp_arg()
        );
    }

    let remaining = layout.backend().list_snapshots()?;
    println!();
    print_snapshot_groups(&config.policy, &remaining);

    match warnings {
        Some(stderr) => Ok(BackupOutcome::Warnings { stderr }),
        None => Ok(BackupOutcome::Clean),
    }
}
