use crate::core::location::Location;
use std::fmt::Write;
use std::path::PathBuf;
use thiserror::Error;

#[derive(Debug, Error)]
pub enum ConfigGeneratorError {
    #[error("failed to write config file at {path}: {reason}")]
    WriteError { path: String, reason: String },
}

#[derive(Debug)]
pub struct ConfigGeneratorParams {
    pub src: Location,
    pub dest: Location,
    pub port: Option<u16>,
    pub id_rsa: Option<PathBuf>,
    pub disable_logs: bool,
    pub log_dir: Option<PathBuf>,
    pub exclude_from: Option<String>,
    pub rsync_flags: String,
}

pub fn generate_config_content(params: &ConfigGeneratorParams) -> String {
    let mut out = String::new();

    writeln!(out, "# JustFolders configuration file").unwrap();
    writeln!(out, "# Edit this file to configure your backup job.").unwrap();
    writeln!(out, "# Lines starting with # are comments and are ignored.").unwrap();
    writeln!(out).unwrap();

    writeln!(out, "# Source directory to back up.").unwrap();
    writeln!(out, "# Accepts: local path, user@host:/path, or SSH alias:/path").unwrap();
    writeln!(out, "src = {}", params.src.original).unwrap();
    writeln!(out).unwrap();

    writeln!(out, "# Destination directory where backups will be stored.").unwrap();
    writeln!(out, "# Accepts: local path, user@host:/path, or SSH alias:/path").unwrap();
    writeln!(out, "dest = {}", params.dest.original).unwrap();
    writeln!(out).unwrap();

    writeln!(out, "# SSH port (optional, default: 22)").unwrap();
    match params.port {
        Some(port) => writeln!(out, "port = {}", port).unwrap(),
        None => writeln!(out, "# port = 22").unwrap(),
    }
    writeln!(out).unwrap();

    writeln!(out, "# Path to SSH private key (optional, uses ~/.ssh/config if not set)").unwrap();
    match &params.id_rsa {
        Some(key) => writeln!(out, "id_rsa = {}", key.display()).unwrap(),
        None => writeln!(out, "# id_rsa = ~/.ssh/backup_key").unwrap(),
    }
    writeln!(out).unwrap();

    writeln!(out, "# Disable logging entirely (optional, default: false)").unwrap();
    writeln!(out, "disable_logs = {}", params.disable_logs).unwrap();

    writeln!(out).unwrap();

    writeln!(out, "# Directory where log files will be stored (optional, default: dest/.logs)").unwrap();
    match &params.log_dir {
        Some(dir) => writeln!(out, "log_dir = {}", dir.display()).unwrap(),
        None => writeln!(out, "# log_dir = /var/log/justfolders").unwrap(),
    }
    writeln!(out).unwrap();

    writeln!(out, "# Path to rsync exclude patterns file (optional)").unwrap();
    match &params.exclude_from {
        Some(exc) => writeln!(out, "exclude_from = {}", exc).unwrap(),
        None => writeln!(out, "# exclude_from = /etc/justfolders/excludes.txt").unwrap(),
    }
    writeln!(out).unwrap();

    writeln!(out, "# Override default rsync flags (optional)").unwrap();
    writeln!(out, "# Default: -D --numeric-ids --links --hard-links --one-file-system").unwrap();
    writeln!(out, "#          --itemize-changes --times --recursive --perms --owner").unwrap();
    writeln!(out, "#          --group --stats --human-readable --delete").unwrap();

    writeln!(out, "rsync_flags = {}", &params.rsync_flags).unwrap();

    out
}

pub fn save_config(content: &str, path: &PathBuf) -> Result<(), ConfigGeneratorError> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(|e| ConfigGeneratorError::WriteError {
            path: path.display().to_string(),
            reason: e.to_string(),
        })?;
    }

    std::fs::write(path, content).map_err(|e| ConfigGeneratorError::WriteError {
        path: path.display().to_string(),
        reason: e.to_string(),
    })?;

    Ok(())
}