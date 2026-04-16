use crate::core::backup_layout::BackupLayout;
use crate::core::config::Config;
use crate::core::notify;
use crate::core::parser::BackupArgs;
use crate::core::retention::apply_policy;
use crate::core::rsync::{Rsync, RsyncError};
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
    fn acquire(backend: &'a dyn StorageBackend) -> Result<Self, StorageBackendError> {
        backend.acquire_lock()?;
        Ok(Self { backend })
    }
}

impl Drop for BackupLock<'_> {
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
}

/// Runs a full backup: loads the config, validates it, runs rsync, and finalizes the snapshot.
///
/// If `[notify]` is configured, an email is sent after every run:
/// - always on failure
/// - on success only when `on_success = true`
pub fn run(args: BackupArgs) -> Result<(), BackupError> {
    let config = Config::load(&args.config)?;

    if config.src.is_remote() && config.dest.is_remote() {
        return Err(BackupError::BothRemote);
    }

    let src = config.src.original.clone();
    let outcome = do_backup(&config);

    if let Some(ref smtp) = config.notify {
        match &outcome {
            Ok(_) => notify::send_backup_result(smtp, true, &src, ""),
            Err(e) => notify::send_backup_result(smtp, false, &src, &e.to_string()),
        }
    }

    outcome
}

fn do_backup(config: &Config) -> Result<(), BackupError> {
    let layout = BackupLayout::new(config);
    layout.prepare_log_dir()?;

    // Acquire lock before any I/O — released automatically when `_lock` is dropped.
    let _lock = BackupLock::acquire(layout.backend())?;

    Rsync::new(config, &layout)?.run()?;

    layout.move_in_progress_to_latest()?;

    let snapshots = layout.backend().list_snapshots()?;
    let to_delete = apply_policy(&config.policy, &snapshots);
    for name in &to_delete {
        layout.backend().delete_snapshot(name)?;
    }

    println!(
        "backup complete: {} → {}",
        config.src.original,
        layout.timestamp_arg()
    );

    Ok(())
}
