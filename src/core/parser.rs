use clap::Parser;
use std::path::PathBuf;

const LONG_ABOUT: &str = "JustFolders is a backup tool built on a simple idea: your data should never depend on special software to be safe.
Instead of hiding files inside proprietary formats or databases, justfolders stores everything as plain directories and files.
Every backup is a real folder you can open, browse, copy, or restore using nothing more than standard system tools.
No lock-in, no decoding step, no surprises.

Backups are organized as snapshots over time, using efficient hard links so unchanged files are not duplicated.
This keeps storage usage low while preserving a complete, point-in-time view of your data.
You get the convenience of modern snapshot backups, with the transparency of a normal filesystem.

The goal is durability. Years from now - on a different machine, or even a different operating system - your backups will still be readable.
If you can navigate folders, you can recover your data. No agents. No proprietary formats. Just folders.";

/// Command-line interface parameters for the JustFolders backup tool.
///
/// This struct defines all supported CLI arguments and flags using `clap`.
/// It supports two main modes:
/// 1. Backup mode (requires `src` and `dest`)
/// 2. Utility mode (`--rsync-get-flags`)
///
/// In backup mode, the tool creates snapshot-style backups without
/// duplicating unchanged files.
#[derive(Parser, Debug)]
#[command(
    name = "JustFolders Backup",
    arg_required_else_help = true,
    version, // auto from Cargo.toml
    about = env!("CARGO_PKG_DESCRIPTION"),
    long_about = LONG_ABOUT
)]
pub struct CliParams {
    /// Source directory to back up.
    ///
    /// Required unless `--rsync-get-flags` is used.
    #[arg(required_unless_present = "rsync_get_flags")]
    pub src: Option<String>,

    /// Destination directory where backups will be stored.
    ///
    /// Required unless `--rsync-get-flags` is used.
    #[arg(required_unless_present = "rsync_get_flags")]
    pub dest: Option<String>,

    /// SSH port used for remote connections.
    #[arg(short = 'p', long = "port", default_value_t = 22, value_name = "PORT")]
    pub ssh_port: u16,

    /// Disable logging entirely.
    #[arg(long = "disable-logs")]
    pub disable_logs: bool,

    /// Directory where log files will be stored. Default destination/.logs
    ///
    /// Defaults to the destination directory if not provided.
    #[arg(long = "log-dir", value_name = "DIR")]
    pub log_dir: Option<PathBuf>,

    /// Path to the private SSH key used for authentication.
    #[arg(short = 'i', long = "id_rsa", value_name = "FILE")]
    pub id_rsa: Option<PathBuf>,

    /// Print the default rsync flags and exit.
    ///
    /// When this flag is present, `src` and `dest` are not required.
    #[arg(long = "rsync-get-flags")]
    pub rsync_get_flags: bool,

    /// Override the default rsync flags used during backup. Example --rsync-set-flags "-avh"
    ///
    /// Accepts a full string of flags. Hyphenated values (e.g. `--links`)
    /// are allowed.
    #[arg(
        long = "rsync-set-flags",
        default_value_t = String::from("-D --numeric-ids --links --hard-links --one-file-system --itemize-changes --times --recursive --perms --owner --group --stats --human-readable"),
        value_name = "FLAGS",
        hide_default_value = true,
        allow_hyphen_values = true
    )]
    pub rsync_set_flags: String,

    /// Append additional rsync flags to the defaults. Example --rsync-append-flags "-vv"
    ///
    /// This does not replace the defaults, but extends them.
    #[arg(
        long = "rsync-append-flags",
        allow_hyphen_values = true,
        value_name = "FLAGS"
    )]
    pub rsync_append_flags: Option<String>,

    /// Automatically delete the oldest backup if disk space runs out.
    // #[arg(long = "delete-oldest-no-space-left")]
    // pub delete_oldest_no_space_left: bool,

    /// Path to a file containing exclude patterns for rsync.
    ///
    /// The file should follow the same format as supported by rsync’s
    /// `--exclude-from` option (one pattern per line).
    ///
    /// This allows excluding files or directories from the backup process.
    ///
    /// See: https://web.archive.org/web/20230126121643/https://sites.google.com/site/rsync2u/home/rsync-tutorial/the-exclude-from-option
    #[arg(long = "exclude-from", value_name = "FILE")]
    pub exclude_from: Option<String>,
}

impl CliParams {
    /// Parse CLI arguments and apply post-processing logic.
    ///
    /// This method:
    /// - Parses arguments using `clap`
    ///
    pub fn from_args() -> CliParams {
        let args = CliParams::parse();
        args
    }

    /// Handle the `--rsync-get-flags` utility mode.
    ///
    /// If enabled, prints the currently configured rsync flags
    /// and exits the program immediately.
    ///
    /// This is intended as a helper/debugging feature so users can
    /// inspect or reuse the default rsync configuration.
    pub fn handle_rsync_get_flags(args: &CliParams) {
        if args.rsync_get_flags {
            println!("{}", args.rsync_set_flags);
            std::process::exit(0);
        }
    }

    /// Checks whether either the source (`src`) or destination (`dest`) paths
    /// represent a remote location in the form of `user@host:/path`.
    ///
    /// This function uses a simple heuristic:
    /// - A remote path is assumed if it contains both `@` and `:`
    ///
    /// Returns `true` if either `src` or `dest` looks like a remote path,
    /// otherwise returns `false`.
    ///
    /// # Notes
    /// - Designed for rsync-style remote specifications.
    /// - Does **not** perform full validation (e.g., IPv6 addresses with `[]` may need special handling).
    /// - Works correctly even if `src` or `dest` is `None`.
    pub fn is_remote(&self) -> bool {
        if let Some(ref src) = self.src
            && src.contains(':')
            && src.contains('@')
        {
            return true;
        }

        if let Some(ref dest) = self.dest
            && dest.contains(':')
            && dest.contains('@')
        {
            return true;
        }

        false
    }

    /// Returns the source path as a `PathBuf`, ensuring it ends with a trailing slash.
    ///
    /// This clones the internal `src` field, panicking if it is `None`.  
    /// If the path does not already end with a `'/'`, one is appended.  
    ///
    /// # Panics
    /// Panics if `self.src` is `None`.
    ///
    /// # Returns
    /// A `PathBuf` representing the normalized source path with a trailing slash.
    pub fn src_path(&self) -> PathBuf {
        let mut src = self.src.clone().unwrap();
        if !src.ends_with('/') {
            src.push('/');
        }

        src.into()
    }

    /// Returns the destination path.
    ///
    /// # Panics
    /// This function will panic if `self.dest` is `None`.
    ///
    /// # Returns
    /// A `PathBuf` representing the destination path, cloned from `self.dest`.
    pub fn dest_path(&self) -> PathBuf {
        self.dest.clone().unwrap().into()
    }
}
