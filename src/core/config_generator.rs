use crate::core::location::Location;
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
    let port = match params.port {
        Some(port) => format!("port = {}", port),
        None => "# port = 22".to_string(),
    };

    let id_rsa = match &params.id_rsa {
        Some(key) => format!("id_rsa = {}", key.display()),
        None => "# id_rsa = ~/.ssh/backup_key".to_string(),
    };

    let log_dir = match &params.log_dir {
        Some(dir) => format!("log_dir = {}", dir.display()),
        None => "# log_dir = /var/log/justfolders".to_string(),
    };

    let exclude_from = match &params.exclude_from {
        Some(exc) => format!("exclude_from = {}", exc),
        None => "# exclude_from = /etc/justfolders/excludes.txt".to_string(),
    };

    let raw = format!(
        "
        # JustFolders configuration file
        # Edit this file to configure your backup job.
        # Lines starting with # are comments and are ignored.
        # Documentation: https://github.com/darkopetreski/justfolders

        # Source directory to back up.
        # Accepts: local path, user@host:/path, or SSH alias:/path
        src = {src}

        # Destination directory where backups will be stored.
        # Accepts: local path, user@host:/path, or SSH alias:/path
        dest = {dest}

        # SSH port (optional, default: 22)
        {port}

        # Path to SSH private key (optional, uses ~/.ssh/config if not set)
        {id_rsa}

        # Disable logging entirely (optional, default: false)
        disable_logs = {disable_logs}

        # Directory where log files will be stored (optional, default: dest/.logs)
        {log_dir}

        # Path to rsync exclude patterns file (optional)
        {exclude_from}

        # Override default rsync flags (optional)
        # Default: -D --numeric-ids --links --hard-links --one-file-system
        #          --itemize-changes --times --recursive --perms --owner
        #          --group --stats --human-readable --delete
        rsync_flags = {rsync_flags}",
        src = params.src.original,
        dest = params.dest.original,
        disable_logs = params.disable_logs,
        rsync_flags = params.rsync_flags,
    );

    raw.lines()
        .map(|line| line.trim_start())
        .collect::<Vec<&str>>()
        .join("\n")
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

pub fn ensure_ini_extension(path: &PathBuf) -> PathBuf {
    match path.extension() {
        Some(ext) if ext == "ini" => path.clone(),
        _ => path.with_extension("ini"),
    }
}
