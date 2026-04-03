mod cli;

use clap::Parser;
use cli::parser::CliParams;

fn main() {
    
    let cli_params = CliParams::parse();
    println!("{:?}", cli_params);
}
