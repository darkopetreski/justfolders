use justfolders::core::parser::{CliParams, Commands, DEFAULT_RSYNC_FLAGS};

fn main() {
    let cli = CliParams::from_args();
    match cli.command {
        Commands::Backup(args) => {
            println!("Run backup now {:?}", args);
        }

        Commands::Info(args) => {
            if args.rsync_get_flags {
                println!("{}", DEFAULT_RSYNC_FLAGS);
            } else if args.display_snapshots.is_some() {
                println!("display snapshots now");
            }
        }
    }
}
