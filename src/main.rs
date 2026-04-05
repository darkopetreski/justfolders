mod backup;
mod cli;

use backup::backup::ensure_marker;
use cli::parser::CliParams;

fn main() {
    // Parse command-line arguments and apply any derived defaults
    let args = CliParams::from_args();

    // Handle utility mode: if `--rsync-get-flags` is provided,
    // print the flags and exit immediately.
    CliParams::handle_rsync_get_flags(&args);

    // Validate that the destination directory is a valid backup target
    // by checking for the presence of the `backup.marker` file.
    //
    // Safe to unwrap because `clap` guarantees `dest` is present
    // unless `--rsync-get-flags` was used (handled above).
    if let Err(err) = ensure_marker(args.dest.as_ref().unwrap()) {
        // Print the error message and terminate with a non-zero exit code
        eprintln!("{}", err);
        std::process::exit(1);
    }

    // Debug output (useful during development)
    // println!("{:?}", args);
}
