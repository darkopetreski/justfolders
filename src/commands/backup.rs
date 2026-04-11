use crate::core::config::Config;
use std::path::PathBuf;

pub fn run(config_path: Option<&PathBuf>) {
    let config = Config::load(config_path).unwrap_or_else(|e| {
        eprintln!("error: {}", e);
        std::process::exit(1);
    });

    // TODO: run rsync
    println!("backing up {} to {}", config.src.original, config.dest.original);
}