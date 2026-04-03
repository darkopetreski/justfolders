mod cli;

use cli::parser::CliParams;

fn main() {
    
    let cli_params = CliParams::get();
    println!("{:?}", cli_params);
}
