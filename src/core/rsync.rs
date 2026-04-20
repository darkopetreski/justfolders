use crate::core::backup_layout::BackupLayout;
use crate::core::config::Config;
use crate::core::storage_backend::StorageBackendError;
use std::io::{self, Read};
use std::process::{Command, ExitStatus, Stdio};
use thiserror::Error;

// ── Errors ───────────────────────────────────────────────────────────────────

/// Errors that can occur when building or executing the rsync process.
#[derive(Debug, Error)]
pub enum RsyncError {
    #[error("failed to execute rsync: {0}")]
    Io(#[from] io::Error),

    #[error("rsync exited with non-zero status ({status}){}", if stderr.is_empty() { String::new() } else { format!("\n{stderr}") })]
    NonZeroStatus { status: ExitStatus, stderr: String },

    #[error("failed to check destination state: {0}")]
    Backend(#[from] StorageBackendError),
}

// ── Rsync ────────────────────────────────────────────────────────────────────

/// A configured `rsync` process ready to execute.
///
/// Construct with [`Rsync::new`], then call [`Rsync::run`] to execute the transfer.
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
    /// stdout is inherited (the user sees transfer progress live).
    /// stderr is captured so error messages can be included in notifications,
    /// then immediately printed to the terminal so nothing is hidden.
    ///
    /// # Errors
    /// - [`RsyncError::NonZeroStatus`] — rsync exited with a non-zero code.
    ///   The captured stderr (e.g. "Permission denied (publickey)") is embedded
    ///   in the error message and will appear in email notifications.
    /// - [`RsyncError::Io`] — failed to spawn the process.
    pub fn run(&mut self) -> Result<(), RsyncError> {
        // Pipe stderr so we can capture it; leave stdout inherited for live progress.
        // rsync error output is always a few lines — no risk of filling the pipe buffer
        // before we read it (we wait for the process first, then drain the pipe).
        self.cmd.stderr(Stdio::piped());

        let mut child = self.cmd.spawn()?;

        // Wait for rsync to finish, then read its stderr.
        let status = child.wait()?;
        let mut stderr = String::new();
        if let Some(mut pipe) = child.stderr.take() {
            pipe.read_to_string(&mut stderr)?;
        }
        let stderr = stderr.trim().to_string();

        // Always mirror stderr to the terminal so the user sees the same output as before.
        if !stderr.is_empty() {
            eprintln!("{stderr}");
        }

        if !status.success() {
            return Err(RsyncError::NonZeroStatus { status, stderr });
        }
        Ok(())
    }

    /// Constructs the rsync [`Command`] from `config` and `layout`.
    ///
    /// Applies flags in this order:
    /// 1. Custom or default rsync flags from config.
    /// 2. `--exclude-from` if set.
    /// 3. `--bwlimit` if set.
    /// 4. `-e ssh …` (with port and optional identity file) when either side is remote.
    /// 5. `--link-dest` pointing to the previous `latest` snapshot, if it exists.
    /// 6. `--log-file` when logging is enabled.
    /// 7. Source and destination arguments.
    ///
    /// Prints the full command to stdout before returning it, so the user can see
    /// exactly what will be executed.
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

        // Bandwidth limit (KB/s). When set, rsync throttles the transfer internally.
        if let Some(bwlimit) = config.bwlimit {
            cmd.arg(format!("--bwlimit={bwlimit}"));
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
        // The log directory is created by the caller before invoking rsync.
        if let Some(log_path) = layout.log_path() {
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
