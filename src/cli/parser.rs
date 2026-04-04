use clap::Parser;
use std::path::PathBuf;

#[derive(Parser, Debug)]
#[command(
    name = "JustFolders Backup",
    arg_required_else_help = true,
    version = "1.0",
    about = "JustFolders is a backup tool for creating folder snapshots without duplicating unchanged files.",
    long_about = include_str!("long_about.txt")
)]
pub struct CliParams {
    pub src: PathBuf,
    pub dest: PathBuf,
    #[arg(
        short = 'p',
        long = "port",
        default_value_t = 22,
        help = "SSH port",
        value_name = "PORT"
    )]
    pub ssh_port: u16,

    #[arg(long = "disable-logs", help = "Disable logging")]
    pub disable_logs: bool,

    #[arg(
        long = "log-dir",
        help = "Directory where to put log files. [default: destination folder]",
        value_name = "DIR"
    )]
    pub log_dir: Option<PathBuf>,

    #[arg(short = 'v', long = "verbose", help = "Verbose")]
    pub verbose: bool,

    #[arg(
        short = 'i',
        long = "id_rsa",
        help = "Specify the private ssh key to use",
        value_name = "FILE"
    )]
    pub id_rsa: Option<PathBuf>,

    #[arg(
        long = "rsync-get-flags",
        help = "Display the default rsync flags that are used for backup."
    )]
    pub rsync_get_flags: bool,

    #[arg(
        long = "rsync-set-flags",
        help = "Set the rsync flags that are going to be used for backup.",
        default_value_t = String::from("-D --numeric-ids --links --hard-links --one-file-system  --itemize-changes --times --recursive --perms --owner  --group --stats --human-readable"),
        value_name = "FLAGS",
        hide_default_value = true,
        allow_hyphen_values = true // allow flags like --links
    )]
    pub rsync_set_flags: String,

    #[arg(
        long = "rsync-append-flags",
        help = "Append the rsync flags, multiple flags as string",
        value_name = "FLAGS"
    )]
    pub rsync_append_flags: Option<String>,

    #[arg(
        long = "delete-oldest-no-space-left",
        help = "Delete the oldest backup to make more space when no space left occurs"
    )]
    pub delete_oldest_no_space_left: bool,
}

///
///
///
impl CliParams {
    ///
    /// Get the cli parameters
    ///
    pub fn from_args() -> CliParams {
        let mut args = CliParams::parse();

        // Set default value for log dir
        if args.log_dir.is_none() {
            args.log_dir = Some(args.dest.clone());
        }

        args
    }
}
