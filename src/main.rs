mod cli;

use cli::parser::CliParams;

fn main() {
    
    let args = CliParams::from_args();
    println!("{:?}", args);
}
