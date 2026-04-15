use crate::core::backup_layout::BackupLayout;
use crate::core::config::Config;
use crate::core::parser::BackupArgs;
use crate::core::rsync::{Rsync, RsyncError};
use crate::core::storage_backend::StorageBackendError;
use thiserror::Error;

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

pub fn run(args: BackupArgs) -> Result<(), BackupError> {
    let config = Config::load(&args.config)?;

    if config.src.is_remote() && config.dest.is_remote() {
        return Err(BackupError::BothRemote);
    }

    let layout = BackupLayout::new(&config);

    Rsync::new(&config, &layout)?.run()?;

    layout.move_in_progress_to_latest()?;

    println!(
        "backup complete: {} → {}",
        config.src.original,
        layout.timestamp_arg()
    );

    Ok(())
}
