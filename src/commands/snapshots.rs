use crate::core::config::Config;
use crate::core::parser::SnapshotsArgs;
use crate::core::storage_backend::from_config;
use thiserror::Error;

#[derive(Debug, Error)]
pub enum SnapshotsError {
    #[error(transparent)]
    Config(#[from] crate::core::config::ConfigError),

    #[error(transparent)]
    Backend(#[from] crate::core::storage_backend::StorageBackendError),
}

pub fn run(args: SnapshotsArgs) -> Result<(), SnapshotsError> {
    let config = Config::load(&args.config)?;

    let backend = from_config(&config);

    let snapshots = backend.list_snapshots()?;
    if snapshots.is_empty() {
        println!("no snapshots found");
    } else {
        snapshots.iter().for_each(|s| println!("{}", s));
    }

    Ok(())
}
