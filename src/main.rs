//! JustFolders binary entry point.
use justfolders::commands;
use justfolders::core::parser::{CliParams, Commands};

fn main() {
    let cli = CliParams::from_args();

    let config_path = cli.config.unwrap_or_else(|| {
        eprintln!("error: --config <FILE> is required");
        eprintln!("usage:");
        eprintln!("  justfolders --config ~/home.ini backup");
        eprintln!("  justfolders backup --config ~/home.ini");
        std::process::exit(1);
    });

    match cli.command {
        Commands::Backup => commands::backup::run(Some(&config_path)),
        Commands::Snapshots => commands::snapshots::run(Some(&config_path)),
        Commands::Init(args) => commands::init::run(args, &config_path),
    }
}
