use crate::core::backup_layout::BackupLayout;
use crate::core::config::Config;
use crate::core::storage_backend::StorageBackendError;
use std::fs::OpenOptions;
use std::io::{self, Read, Write};
use std::path::PathBuf;
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

/// Outcome of a successful (or partially successful) rsync invocation.
///
/// rsync's exit code carries severity information: code 0 means the entire
/// transfer succeeded, while codes 23 and 24 mean some files were skipped
/// (typically permission denied on individual files, or files that vanished
/// mid-transfer) but the rest of the transfer completed normally.  Any other
/// non-zero code is treated as a hard failure and produces [`RsyncError`].
#[derive(Debug)]
pub enum RsyncOutcome {
    /// rsync exited with code 0 — every file was transferred.
    Clean,
    /// rsync exited with code 23 or 24 — some files were not transferred but
    /// the rest of the run succeeded.  `code` is the actual rsync exit code
    /// and `stderr` is the captured rsync stderr (already mirrored to the
    /// terminal) so the caller can include it in a notification email.
    Warnings { code: i32, stderr: String },
}

// ── Rsync ────────────────────────────────────────────────────────────────────

/// A configured `rsync` process ready to execute.
///
/// Construct with [`Rsync::new`], then call [`Rsync::run`] to execute the transfer.
pub struct Rsync {
    cmd: Command,
    /// Path of the `--log-file` rsync was told to write, if logging is enabled.
    /// Captured here so [`run`] can append the captured stderr to the same file
    /// after rsync exits — rsync's own log does not include stderr messages
    /// (especially those forwarded from the remote sender over SSH), so without
    /// this the log file would be missing things like "permission denied" that
    /// the user sees on the terminal.
    log_path: Option<PathBuf>,
}

impl Rsync {
    /// Builds an `rsync` command from [`Config`] and [`BackupLayout`].
    pub fn new(config: &Config, layout: &BackupLayout) -> Result<Self, RsyncError> {
        Ok(Self {
            cmd: Self::build_cmd(config, layout)?,
            log_path: layout.log_path(),
        })
    }

    /// Executes the configured `rsync` command.
    ///
    /// stdout is inherited (the user sees transfer progress live).
    /// stderr is captured so error messages can be included in notifications,
    /// then immediately printed to the terminal so nothing is hidden.
    ///
    /// # Returns
    /// - [`RsyncOutcome::Clean`] — rsync exited cleanly (code 0).
    /// - [`RsyncOutcome::Warnings`] — rsync exited with code 23 or 24, meaning
    ///   some files were not transferred (typically permission denied on
    ///   individual files, or files that vanished mid-transfer) but the rest
    ///   of the transfer completed normally.
    ///
    /// # Errors
    /// - [`RsyncError::NonZeroStatus`] — rsync exited with any other non-zero
    ///   code (real protocol, I/O, or transport errors).  The captured stderr
    ///   (e.g. "Permission denied (publickey)") is embedded in the error
    ///   message and will appear in email notifications.
    /// - [`RsyncError::Io`] — failed to spawn the process.
    pub fn run(&mut self) -> Result<RsyncOutcome, RsyncError> {
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

        // Append stderr to the rsync log file as well.
        //
        // rsync's `--log-file` only records what the local rsync process itself
        // logs.  Messages written directly to stderr — most notably
        // "permission denied" / "failed to open" errors that come from the
        // *remote* sender and are forwarded over SSH — never reach rsync's own
        // logger, so they end up on the user's terminal but are missing from
        // the log file.  Appending the captured stderr here keeps the log file
        // in sync with what the user actually saw.
        //
        // Failure to write to the log is intentionally non-fatal: the transfer
        // itself succeeded (or failed) on its own merits and we don't want a
        // log I/O hiccup to mask that outcome.
        if !stderr.is_empty() {
            if let Some(ref log_path) = self.log_path {
                if let Err(e) = append_stderr_to_log(log_path, &stderr) {
                    eprintln!(
                        "warning: failed to append rsync stderr to {}: {e}",
                        log_path.display()
                    );
                }
            }
        }

        // rsync exit codes 23 and 24 indicate a *partial* transfer:
        //   23 — some files could not be transferred (e.g. permission denied on
        //        individual files).  The rest of the transfer is intact.
        //   24 — some source files vanished during the transfer (common on live
        //        systems with rotating logs, cache files, etc.).
        // These are reported as warnings rather than errors so the caller can
        // choose, via the `strict` config flag, whether to still create the
        // snapshot or to escalate them to a full failure.  Any other non-zero
        // exit code is a real failure.
        match status.code() {
            Some(0) => Ok(RsyncOutcome::Clean),
            Some(code @ (23 | 24)) => Ok(RsyncOutcome::Warnings { code, stderr }),
            _ => Err(RsyncError::NonZeroStatus { status, stderr }),
        }
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

/// Appends rsync's captured stderr to the log file rsync was told to write.
///
/// A short header marks the section so the stderr block is easy to spot when
/// scanning the log.  The file is opened in append mode and created if rsync
/// produced stderr without producing any log output of its own (e.g. when
/// rsync failed before opening its log file).
fn append_stderr_to_log(log_path: &PathBuf, stderr: &str) -> io::Result<()> {
    let mut file = OpenOptions::new()
        .create(true)
        .append(true)
        .open(log_path)?;
    writeln!(file, "\n--- rsync stderr ---\n{stderr}")?;
    Ok(())
}
