use crate::core::location::Location;
use clap::{Parser, Subcommand};
use std::path::PathBuf;

//pub const DEFAULT_RSYNC_FLAGS: &str = "-D --numeric-ids --links --hard-links --one-file-system --itemize-changes --times --recursive --perms --owner --group --stats --human-readable --delete";

const LONG_ABOUT: &str = "JustFolders is a backup tool built on a simple idea: your data should never depend on special software to be safe.
Instead of hiding files inside proprietary formats or databases, justfolders stores everything as plain directories and files.
Every backup is a real folder you can open, browse, copy, or restore using nothing more than standard system tools.
No lock-in, no decoding step, no surprises.

Backups are organized as snapshots over time, using efficient hard links so unchanged files are not duplicated.
This keeps storage usage low while preserving a complete, point-in-time view of your data.
You get the convenience of modern snapshot backups, with the transparency of a normal filesystem.

The goal is durability. Years from now - on a different machine, or even a different operating system - your backups will still be readable.
If you can navigate folders, you can recover your data. No agents. No proprietary formats. Just folders.";

/// Root CLI entry point for JustFolders.
#[derive(Parser, Debug)]
#[command(
    name = "justfolders",
    arg_required_else_help = true,
    version,
    about = env!("CARGO_PKG_DESCRIPTION"),
    long_about = LONG_ABOUT
)]
pub struct CliParams {
    #[command(subcommand)]
    pub command: Commands,
}
#[derive(Subcommand, Debug)]
pub enum Commands {
    /// Run a backup job using the config file.
    Backup(BackupArgs),

    /// List all available snapshots at the destination.
    Snapshots(SnapshotsArgs),

    /// Generate a config file from provided arguments.
    Init(InitArgs),
}

#[derive(Parser, Debug)]
pub struct BackupArgs {
    /// Path to the config file.
    #[arg(long = "config", value_name = "FILE", required = true)]
    pub config: PathBuf,
}

#[derive(Parser, Debug)]
pub struct SnapshotsArgs {
    /// Path to the config file.
    #[arg(long = "config", value_name = "FILE", required = true)]
    pub config: PathBuf,
}

#[derive(Parser, Debug)]
pub struct InitArgs {
    /// Path to the config file to generate.
    #[arg(long = "config", value_name = "FILE", required = true)]
    pub config: PathBuf,

    /// Source directory to back up.
    #[arg(long = "src", required = true)]
    pub src: Location,

    /// Destination directory where backups will be stored.
    #[arg(long = "dest", required = true)]
    pub dest: Location,

    /// SSH port used for remote connections.
    #[arg(short = 'p', long = "port", value_name = "PORT")]
    pub ssh_port: Option<u16>,

    /// Path to the private SSH key used for authentication.
    #[arg(short = 'i', long = "id-rsa", value_name = "FILE")]
    pub id_rsa: Option<PathBuf>,

    /// Disable logging entirely.
    #[arg(long = "disable-logs")]
    pub disable_logs: Option<bool>,

    /// Directory where log files will be stored.
    #[arg(long = "log-dir", value_name = "DIR")]
    pub log_dir: Option<PathBuf>,

    /// Path to a file containing exclude patterns for rsync.
    #[arg(long = "exclude-from", value_name = "FILE")]
    pub exclude_from: Option<String>,

    /// Print the config to stdout instead of saving it.
    #[arg(long = "dry-run")]
    pub dry_run: bool,
}

impl CliParams {
    /// Parses CLI arguments using clap and returns a populated [`CliParams`].
    pub fn from_args() -> CliParams {
        CliParams::parse()
    }
}
