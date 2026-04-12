use justfolders::commands;
use justfolders::core::parser::{CliParams, Commands};

fn main() {
    let cli = CliParams::from_args();

    let result = match cli.command {
        Commands::Backup(args) => commands::backup::run(args).map_err(|e| e.to_string()),
        Commands::Snapshots(args) => commands::snapshots::run(args).map_err(|e| e.to_string()),
        Commands::Init(args) => commands::init::run(args).map_err(|e| e.to_string()),
    };

    if let Err(e) = result {
        eprintln!("error: {}", e);
        std::process::exit(1);
    }
}
