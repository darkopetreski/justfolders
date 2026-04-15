use crate::core::backup_layout::BackupLayout;
use crate::core::config::Config;
use crate::core::storage_backend::StorageBackendError;
use std::fs;
use std::io;
use std::process::{Command, ExitStatus};
use thiserror::Error;

// ── Errors ───────────────────────────────────────────────────────────────────

#[derive(Debug, Error)]
pub enum RsyncError {
    #[error("failed to execute rsync: {0}")]
    Io(#[from] io::Error),

    #[error("rsync exited with non-zero status ({status})")]
    NonZeroStatus { status: ExitStatus },

    #[error("failed to check destination state: {0}")]
    Backend(#[from] StorageBackendError),
}

// ── Rsync ────────────────────────────────────────────────────────────────────

pub struct Rsync {
    cmd: Command,
}

impl Rsync {
    /// Builds an `rsync` command from [`Config`] and [`BackupLayout`].
    pub fn new(config: &Config, layout: &BackupLayout) -> Result<Self, RsyncError> {
        Ok(Self {
            cmd: Self::build_cmd(config, layout)?,
        })
    }

    /// Executes the configured `rsync` command.
    ///
    /// # Errors
    /// - [`RsyncError::NonZeroStatus`] — rsync exited with a non-zero code.
    /// - [`RsyncError::Io`] — failed to spawn the process.
    pub fn run(&mut self) -> Result<(), RsyncError> {
        let status = self.cmd.status()?;
        if !status.success() {
            return Err(RsyncError::NonZeroStatus { status });
        }
        Ok(())
    }

    fn build_cmd(config: &Config, layout: &BackupLayout) -> Result<Command, RsyncError> {
        let mut cmd = Command::new("rsync");

        // Default or custom flags
        if let Some(ref flags) = config.rsync_flags {
            cmd.args(flags.split_whitespace());
        }

        // Exclude patterns file
        if let Some(ref file) = config.exclude_from {
            cmd.arg("--exclude-from").arg(file);
        }

        // SSH transport: port + optional identity file.
        // The key path is single-quoted so spaces in the path are handled correctly.
        if config.src.is_remote() || config.dest.is_remote() {
            let mut ssh_cmd = format!("ssh -p {}", config.port);
            if let Some(ref key) = config.id_rsa {
                ssh_cmd.push_str(&format!(" -i '{}'", key.display()));
            }
            cmd.arg("-e").arg(ssh_cmd);
            cmd.arg("--compress");
        }

        // Hard-link unchanged files against the previous snapshot only when
        // `latest` actually exists (skipped on the very first run).
        if layout.backend().latest_exists()? {
            cmd.arg("--link-dest").arg(layout.link_dest_arg());
        }

        // Log file is always local (rsync writes it on the machine it runs on).
        if let Some(log_path) = layout.log_path() {
            if let Some(parent) = log_path.parent() {
                fs::create_dir_all(parent).ok();
            }
            cmd.arg("--log-file").arg(log_path);
        }

        cmd.arg(&config.src.original);
        cmd.arg(layout.in_progress_arg());

        // Intentional: prints the full rsync invocation before execution so the
        // user can see exactly what is being run. Do not remove.
        println!("rsync command: {:?}", cmd);

        Ok(cmd)
    }
}
