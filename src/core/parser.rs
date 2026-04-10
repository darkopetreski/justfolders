use crate::core::location::Location;
use clap::{Parser, Subcommand};
use std::path::PathBuf;

pub const DEFAULT_RSYNC_FLAGS: &str = "-D --numeric-ids --links --hard-links --one-file-system --itemize-changes --times --recursive --perms --owner --group --stats --human-readable --delete";

const LONG_ABOUT: &str = "JustFolders is a backup tool built on a simple idea: your data should never depend on special software to be safe.
Instead of hiding files inside proprietary formats or databases, justfolders stores everything as plain directories and files.
Every backup is a real folder you can open, browse, copy, or restore using nothing more than standard system tools.
No lock-in, no decoding step, no surprises.

Backups are organized as snapshots over time, using efficient hard links so unchanged files are not duplicated.
This keeps storage usage low while preserving a complete, point-in-time view of your data.
You get the convenience of modern snapshot backups, with the transparency of a normal filesystem.

The goal is durability. Years from now - on a different machine, or even a different operating system - your backups will still be readable.
If you can navigate folders, you can recover your data. No agents. No proprietary formats. Just folders.";

#[derive(Parser, Debug)]
#[command(
    name = "JustFolders Backup",
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
    /// Run a backup from source to destination.
    Backup(BackupArgs),

    /// Print information, only one option can be used at a time.
    Info(InfoArgs),
}

#[derive(Parser, Debug)]
#[command(arg_required_else_help = true)]
pub struct BackupArgs {
    /// Source directory to back up. [user@host:]/path
    pub src: Location,

    /// Destination directory where backups will be stored. [user@host:]/path
    pub dest: Location,

    /// SSH port used for remote connections.
    #[arg(short = 'p', long = "port", default_value_t = 22, value_name = "PORT")]
    pub ssh_port: u16,

    /// Disable logging entirely.
    #[arg(long = "disable-logs")]
    pub disable_logs: bool,

    /// Directory where log files will be stored. Default destination/.logs
    #[arg(long = "log-dir", value_name = "DIR")]
    pub log_dir: Option<PathBuf>,

    /// Path to the private SSH key used for authentication.
    #[arg(short = 'i', long = "id_rsa", value_name = "FILE")]
    pub id_rsa: Option<PathBuf>,

    /// Override the default rsync flags used during backup.
    #[arg(
        long = "rsync-set-flags",
        default_value_t = String::from(DEFAULT_RSYNC_FLAGS),
        value_name = "FLAGS",
        hide_default_value = true,
        allow_hyphen_values = true
    )]
    pub rsync_set_flags: String,

    /// Append additional rsync flags to the defaults.
    #[arg(
        long = "rsync-append-flags",
        allow_hyphen_values = true,
        value_name = "FLAGS"
    )]
    pub rsync_append_flags: Option<String>,

    /// Path to a file containing exclude patterns for rsync.
    #[arg(long = "exclude-from", value_name = "FILE")]
    pub exclude_from: Option<String>,
}

#[derive(Parser, Debug)]
#[command(
    arg_required_else_help = true,
    override_usage = "justfolders info [OPTION]"
)]
#[group(multiple = false, required = true)]
pub struct InfoArgs {
    /// Print the default rsync flags.
    #[arg(long = "rsync-get-flags")]
    pub rsync_get_flags: bool,

    /// List all available snapshots
    #[arg(long = "display-snapshots")]
    pub display_snapshots: bool,
}

impl CliParams {
    /// Parse CLI arguments and apply post-processing logic.
    ///
    /// This method:
    /// - Parses arguments using `clap`
    ///
    pub fn from_args() -> CliParams {
        let cli = CliParams::parse();
        // post-processing or validation here
        cli
    }
}
