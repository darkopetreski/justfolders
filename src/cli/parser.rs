use clap::Parser;

#[derive(Parser, Debug)]
#[command(
    name = "JustFolders Backup",
    arg_required_else_help = true,
    version = "1.0",
    about = "JustFolders is a backup tool for creating folder snapshots without duplicating unchanged files.",
    long_about = "JustFolders is a backup tool built on a simple idea: your data should never depend on special software to be safe.\n
Instead of hiding files inside proprietary formats or databases, justfolders stores everything as plain directories and files. 
Every backup is a real folder you can open, browse, copy, or restore using nothing more than standard system tools. 
No lock-in, no decoding step, no surprises.\n
Backups are organized as snapshots over time, using efficient hard links so unchanged files aren’t duplicated. 
This keeps storage usage low while preserving a complete, point-in-time view of your data. 
You get the convenience of modern snapshot backups, with the transparency of a normal filesystem.\n
The goal is durability. Years from now—on a different machine, or even a different operating system—your backups will still be readable. 
If you can navigate folders, you can recover your data. No agents. No proprietary formats. Just folders."
)]
pub struct CliParams {
    pub src: String,
    pub dest: String,
    #[arg(
        short = 'p',
        long = "port",
        default_value_t = 22,
        help = "SSH port",
        value_name = "PORT"
    )]
    pub ssh_port: u16,

    #[arg(
        long = "disable-logs",
        default_value_t = false,
        help = "Disable logging"
    )]
    pub disable_logs: bool,

    #[arg(
        short = 'v',
        long = "verbose",
        default_value_t = false,
        help = "Verbose"
    )]
    pub verbose: bool,

    #[arg(
        short = 'i',
        long = "id_rsa",
        help = "Specify the private ssh key to use",
        value_name = "FILE"
    )]
    pub id_rsa: Option<String>,

    #[arg(
        long = "rsync-get-flags",
        help = "Display the default rsync flags that are used for backup."
    )]
    pub rsync_get_flags: bool,

    #[arg(
        long = "rsync-set-flags",
        help = "Set the rsync flags that are going to be used for backup.",
        value_name = "FLAGS"
    )]
    pub rsync_set_flags: Option<String>,

    #[arg(
        long = "rsync-append-flags",
        help = "Append the rsync flags",
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
    pub fn get() -> CliParams {
        CliParams::parse()
    }
}
