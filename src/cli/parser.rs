use clap::Parser;

#[derive(Parser, Debug)]
#[command(version = "1.0")]
pub struct CliParams {
    pub src: String,
    pub dest: String,
    #[arg(short = 'p', long = "port", default_value_t = 22)]
    pub ssh_port: u16,
}
