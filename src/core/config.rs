use crate::core::location::Location;
use configparser::ini::Ini;
use std::path::PathBuf;
use thiserror::Error;

// ── Errors ─────────────────────────────────────────────────────────────────

#[derive(Debug, Error)]
pub enum ConfigError {
    #[error("config file not found at {0}")]
    NotFound(String),

    #[error("failed to read config file at {path}: {reason}")]
    ReadError { path: String, reason: String },

    #[error("failed to write config file at {path}: {reason}")]
    WriteError { path: String, reason: String },

    #[error("missing required field '{0}' in config file")]
    MissingField(String),

    #[error("invalid value for '{field}': {reason}")]
    InvalidValue { field: String, reason: String },
}

// ── Config struct ──────────────────────────────────────────────────────────

#[derive(Debug)]
pub struct Config {
    pub src: Location,
    pub dest: Location,
    pub port: u16,
    pub id_rsa: Option<PathBuf>,
    pub disable_logs: bool,
    pub log_dir: Option<PathBuf>,
    pub exclude_from: Option<String>,
    pub rsync_flags: Option<String>,
}

impl Config {
    /// Load config from a path. If the path is just a name (no directory
    /// component), look for it in the current directory.
    pub fn load(path: &PathBuf) -> Result<Self, ConfigError> {
        let path = resolve_path(path);

        let mut ini = Ini::new();
        ini.load(&path).map_err(|e| ConfigError::ReadError {
            path: path.display().to_string(),
            reason: e,
        })?;

        let src_str = ini
            .get("default", "src")
            .ok_or_else(|| ConfigError::MissingField("src".to_string()))?;

        let dest_str = ini
            .get("default", "dest")
            .ok_or_else(|| ConfigError::MissingField("dest".to_string()))?;

        let src = src_str
            .parse::<Location>()
            .map_err(|e| ConfigError::InvalidValue {
                field: "src".to_string(),
                reason: e.to_string(),
            })?;

        let dest = dest_str
            .parse::<Location>()
            .map_err(|e| ConfigError::InvalidValue {
                field: "dest".to_string(),
                reason: e.to_string(),
            })?;

        let port = ini
            .getuint("default", "port")
            .map_err(|e| ConfigError::InvalidValue {
                field: "port".to_string(),
                reason: e,
            })?
            .unwrap_or(22) as u16;

        let id_rsa = ini.get("default", "id_rsa").map(PathBuf::from);
        let log_dir = ini.get("default", "log_dir").map(PathBuf::from);
        let exclude_from = ini.get("default", "exclude_from");
        let rsync_flags = ini.get("default", "rsync_flags");

        let disable_logs = ini
            .getbool("default", "disable_logs")
            .map_err(|e| ConfigError::InvalidValue {
                field: "disable_logs".to_string(),
                reason: e,
            })?
            .unwrap_or(false);

        Ok(Config {
            src,
            dest,
            port,
            id_rsa,
            disable_logs,
            log_dir,
            exclude_from,
            rsync_flags,
        })
    }
}

// ── Generator params ───────────────────────────────────────────────────────

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

// ── Generation ─────────────────────────────────────────────────────────────

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

// ── Saving ─────────────────────────────────────────────────────────────────

pub fn save_config(content: &str, path: &PathBuf) -> Result<(), ConfigError> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(|e| ConfigError::WriteError {
            path: path.display().to_string(),
            reason: e.to_string(),
        })?;
    }

    std::fs::write(path, content).map_err(|e| ConfigError::WriteError {
        path: path.display().to_string(),
        reason: e.to_string(),
    })?;

    Ok(())
}

// ── Path helpers ───────────────────────────────────────────────────────────

/// If the path has no directory component, resolve it relative to the
/// current directory. Otherwise use the full path as provided.
pub fn resolve_path(path: &PathBuf) -> PathBuf {
    let with_ext = ensure_ini_extension(path);
    if with_ext.components().count() == 1 {
        // just a filename, no directory — look in current directory
        std::env::current_dir()
            .unwrap_or_else(|_| PathBuf::from("."))
            .join(with_ext)
    } else {
        with_ext
    }
}

pub fn ensure_ini_extension(path: &PathBuf) -> PathBuf {
    match path.extension() {
        Some(ext) if ext == "ini" => path.clone(),
        _ => path.with_extension("ini"),
    }
}
