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



                use justfolders::core::storage_backend::from_location;
                let backend = from_location(&args.display_snapshots.unwrap(), 22, None);
                match backend.list_snapshots() {
                    Ok(snapshots) => snapshots.iter().for_each(|s| println!("{}", s)),
                    Err(e) => eprintln!("error: {}", e),
                }


            }
        }
    }
}
