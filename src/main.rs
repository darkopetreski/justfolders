use justfolders::commands;
use justfolders::core::parser::{CliParams, Commands};

/// Entry point: parse CLI arguments and dispatch to the appropriate command handler.
///
/// All commands return a `Result<(), E: Display>`.  Errors are mapped to `String`
/// so they can be printed uniformly, and the process exits with code 1 on failure.
fn main() {
    let cli = CliParams::from_args();
    let result = match cli.command {
        Commands::Backup(args) => commands::backup::run(args).map_err(|e| e.to_string()),
        Commands::Snapshots(args) => commands::snapshots::run(args).map_err(|e| e.to_string()),
        Commands::Init(args) => commands::init::run(args).map_err(|e| e.to_string()),
        Commands::TestNotify(args) => commands::test_notify::run(args).map_err(|e| e.to_string()),
        Commands::Verify(args) => commands::verify::run(args).map_err(|e| e.to_string()),
    };

    if let Err(e) = result {
        eprintln!("error: {}", e);
        std::process::exit(1);
    }
}
