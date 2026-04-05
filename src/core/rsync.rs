use std::io;
use std::process::{Command, ExitStatus};
use thiserror::Error;

use super::parser::CliParams;

#[derive(Debug, Error)]
pub enum RsyncError {
    #[error("failed to execute command: {0}")]
    Io(#[from] io::Error),

    #[error("command exited unsuccessfully ({status})")]
    NonZeroStatus { status: ExitStatus },
}

pub struct Rsync {
    cmd: Command,
}

impl Rsync {
    /// Builds an `rsync` invocation from parsed CLI arguments.
    pub fn new(_params: &CliParams) -> Self {
        Self {
            cmd: Command::new("rsync"),
        }
    }

    pub fn run(&mut self) -> Result<(), RsyncError> {
        let status = self.cmd.status()?;
        if status.success() {
            Ok(())
        } else {
            Err(RsyncError::NonZeroStatus { status })
        }
    }
}
