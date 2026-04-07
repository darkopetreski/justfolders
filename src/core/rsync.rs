use crate::core::backup::Backup;

use super::parser::CliParams;
use std::io;
use std::path::PathBuf;
use std::process::{Command, ExitStatus};
use thiserror::Error;

/// Errors that can occur when building or running an `rsync` command.
#[derive(Debug, Error)]
pub enum RsyncError {
    /// Error from the underlying IO operations when executing `rsync`.
    #[error("failed to execute command: {0}")]
    Io(#[from] io::Error),

    /// `rsync` exited with a non-zero status code.
    #[error("command exited unsuccessfully ({status})")]
    NonZeroStatus { status: ExitStatus },

    /// Source path (`src`) is missing from the CLI parameters
    #[error("missing source path")]
    MissingSrc,

    /// Destination path (`dest`) is missing from the CLI parameters.
    #[error("missing destination path")]
    MissingDest,
}

pub struct Rsync {
    cmd: Command,
}

impl Rsync {
    /// Constructs a new `Rsync` instance from parsed CLI parameters and an optional subfolder.
    ///
    /// # Arguments
    /// - `params`: Reference to parsed CLI parameters.
    /// - `subfolder`: Subfolder to append to the destination path.
    ///
    /// # Returns
    /// - `Ok(Self)` if the command was built successfully.
    /// - `Err(RsyncError)` if required parameters (`src` or `dest`) are missing.
    ///
    pub fn new(params: &CliParams, backup: &Backup) -> Result<Self, RsyncError> {
        Ok(Self {
            cmd: Self::build_cmd(params, backup)?,
            //backup: backup,
        })
    }

    /// Executes the configured `rsync` command.
    ///
    /// # Returns
    /// - `Ok(())` if the command executed successfully (exit code 0).
    /// - `Err(RsyncError::NonZeroStatus)` if `rsync` exited with a non-zero code.
    /// - `Err(RsyncError::Io)` if an IO error occurred while spawning the process.
    ///
    pub fn run(&mut self) -> Result<(), RsyncError> {
        let status = self.cmd.status()?;
        if !status.success() {
            return Err(RsyncError::NonZeroStatus { status });
        }

        Ok(())
    }

    /// Builds the `rsync` `Command` from the provided CLI parameters.
    ///
    /// Handles:
    /// - Default and additional flags
    /// - `--exclude-from` file
    /// - SSH options including port and optional private key
    /// - Verbosity
    /// - Source and destination paths (with subfolder appended)
    ///
    /// # Arguments
    /// - `params`: CLI parameters parsed from user input.
    /// - `subfolder`: Subfolder to append to the destination path.
    ///
    /// # Errors
    /// - `RsyncError::MissingSrc` if `src` is missing.
    /// - `RsyncError::MissingDest` if `dest` is missing.
    ///
    fn build_cmd(params: &CliParams, backup: &Backup) -> Result<Command, RsyncError> {
        let mut cmd = Command::new("rsync");

        // Set the default or custom flags
        cmd.args(params.rsync_set_flags.split_whitespace());

        // Add exclude from file
        if let Some(ref file) = params.exclude_from {
            cmd.arg("--exclude-from").arg(file);
        }

        // SSH port and optional private key
        if params.is_remote() {
            // Start with base SSH command and port
            let mut ssh_cmd = format!("ssh -p {}", params.ssh_port);

            // If private key is provided, add -i
            if let Some(ref key_path) = params.id_rsa {
                ssh_cmd.push_str(&format!(" -i {}", key_path.display()));
            }

            cmd.arg("-e").arg(ssh_cmd);
            cmd.arg("--compress");
        }

        // Add additional flags
        if let Some(ref append_flags) = params.rsync_append_flags {
            cmd.args(append_flags.split_whitespace());
        }

        // If there is a previous backup, create hard links for unchanged files
        if let Some(previous_dest) = backup.get_latest_folder() {
            cmd.arg("--link-dest").arg(previous_dest);
        }

        // SRC, DEST with proper error handling
        let src = params.src.as_ref().ok_or(RsyncError::MissingSrc)?;
        let dest = params.dest.as_ref().ok_or(RsyncError::MissingDest)?;

        // Force trailing '/' on source to always copy contents
        let mut src_path = src.clone().to_string();
        if !src_path.ends_with('/') {
            src_path.push('/');
        }

        cmd.arg(src_path);
        cmd.arg(PathBuf::from(dest).join(backup.in_progress_folder_name()));

        println!(
            "Running rsync: {} {}",
            cmd.get_program().to_string_lossy(),
            cmd.get_args()
                .map(|a| a.to_string_lossy().into_owned())
                .collect::<Vec<_>>()
                .join(" ")
        );

        //TODO implement logs in file

        // disable_logs
        // log_dir

        Ok(cmd)
    }
}
