use crate::core::config::Config;
use crate::core::parser::BackupArgs;
use thiserror::Error;

#[derive(Debug, Error)]
pub enum BackupError {
    #[error(transparent)]
    Config(#[from] crate::core::config::ConfigError),
}

pub fn run(args: BackupArgs) -> Result<(), BackupError> {
    let config = Config::load(&args.config)?;
    // TODO: run rsync
    println!("backing up {} to {}", config.src.original, config.dest.original);
    Ok(())
}