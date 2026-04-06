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

    fn build_cmd(params: &CliParams, subfolder: String) -> Command {
        let mut cmd = Command::new("rsync");

        // Set the default or custom flags
        cmd.args(params.rsync_set_flags.split_whitespace());

        // Add exclude from file
        if let Some(ref file) = params.exclude_from {
            cmd.arg("--exclude-from").arg(file);
        }

        // SSH port
        cmd.arg("-e").arg(format!("ssh -p {}", params.ssh_port));
        cmd.arg("--compress");

        // Add additional flags
        if let Some(ref append_flags) = params.rsync_append_flags {
            cmd.args(append_flags.split_whitespace());
        }

        // Verbose
        if params.verbose {
            cmd.arg("-v");
        }

        // SRC, DEST
        cmd.arg(params.src.as_ref().unwrap());
        //cmd.arg(params.dest.as_ref().);

        //println!("{}", params.src.clone().unwrap().to_string_lossy());

        /**

            pub dest: Option<PathBuf>,

            pub disable_logs: bool,
            pub log_dir: Option<PathBuf>,
            pub id_rsa: Option<PathBuf>,


        */
        cmd
    }
}
