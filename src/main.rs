mod cli;

use cli::parser::CliParams;

fn main() {
    let args = CliParams::from_args();
    CliParams::handle_rsync_get_flags(&args);

    println!("{:?}", args);
}

