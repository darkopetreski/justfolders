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
    pub fn new(params: &CliParams) -> Self {
        Self {
            cmd: Self::build_cmd(params),
        }
    }

    pub fn run(&mut self) -> Result<(), RsyncError> {
        let status = self.cmd.status()?;
        if !status.success() {
            return Err(RsyncError::NonZeroStatus { status });
        }
        
        Ok(())
    }

    fn build_cmd(params: &CliParams) -> Command {
        let mut cmd = Command::new("rsync");
        cmd.args(params.rsync_set_flags.split_whitespace());
        // cmd.args(params.rsync_append_flags.iter().map(|s| s.as_str()));
        // cmd.args(params.exclude_from.iter().map(|s| s.as_str()));
        // cmd.args(params.rsync_get_flags.iter().map(|s| s.as_str()));
        // cmd.args(params.rsync_set_flags.split_whitespace());
        // cmd.args(params.rsync_append_flags.iter().map(|s| s.as_str()));
        // cmd.args(params.exclude_from.iter().map(|s| s.as_str()));
        // cmd.args(params.rsync_get_flags.iter().map(|s| s.as_str()));
        cmd
    }
}
