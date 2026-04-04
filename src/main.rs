mod cli;

use cli::parser::CliParams;

fn main() {
    
    let args = CliParams::get();
    println!("{:?}", args);
}
