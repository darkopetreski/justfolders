use crate::core::config::Config;
use crate::core::storage_backend::from_location;
use std::path::PathBuf;

pub fn run(config_path: Option<&PathBuf>) {
    let config = Config::load(config_path).unwrap_or_else(|e| {
        eprintln!("error: {}", e);
        std::process::exit(1);
    });

    let backend = from_location(&config.dest, config.port, config.id_rsa);

    match backend.list_snapshots() {
        Ok(snapshots) if snapshots.is_empty() => println!("no snapshots found"),
        Ok(snapshots) => snapshots.iter().for_each(|s| println!("{}", s)),
        Err(e) => {
            eprintln!("error: {}", e);
            std::process::exit(1);
        }
    }
}