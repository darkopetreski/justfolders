//! JustFolders binary entry: parse CLI, handle `--rsync-get-flags`, validate the
//! backup destination, then run rsync.

mod core;

use core::backup::Backup;
use core::parser::CliParams;
use core::rsync::Rsync;

fn main() {
    // Parse arguments with clap and apply derived defaults (e.g. log dir → dest).
    let args = CliParams::from_args();

    // Utility mode: print the configured rsync flags and exit. Skips backup work.
    CliParams::handle_rsync_get_flags(&args);

    let backup = Backup::new(&args);

    // Require `backup.marker` under the destination so we do not run against the wrong tree.
    if let Err(err) = backup.ensure_marker() {
        eprintln!("{}", err);
        std::process::exit(1);
    }

    // Run the rsync invocation built from CLI flags.
    let mut rsync = Rsync::new(&args);
    if let Err(err) = rsync.run() {
        eprintln!("{}", err);
        std::process::exit(1);
    }

    // println!("{:?}", args); // uncomment to inspect parsed CLI during development
}
