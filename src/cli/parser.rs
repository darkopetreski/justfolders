use clap::{Parser};

#[derive(Parser, Debug)]
pub struct CliParams {
    pub src: String,
    pub dest: String,
    #[arg(short = 'p', long = "port", default_value_t = 22)]
    pub ssh_port: u16,
}

///
///
///
impl CliParams {
    ///
    /// Get the cli parameters
    ///
    pub fn get() -> CliParams {
        CliParams::parse()
    }
}
