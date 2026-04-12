use crate::core::location::Location;
use configparser::ini::Ini;
use std::path::PathBuf;
use thiserror::Error;

#[derive(Debug, Error)]
pub enum ConfigError {
    #[error("config file not found, run 'justfolders init' to create one")]
    NotFound,

    #[error("failed to read config file at {path}: {reason}")]
    ReadError { path: String, reason: String },

    #[error("missing required field '{0}' in config file")]
    MissingField(String),

    #[error("invalid value for '{field}': {reason}")]
    InvalidValue { field: String, reason: String },
}

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
    /// Load config from an explicit path or auto-discover it.
    pub fn load(path: &PathBuf) -> Result<Self, ConfigError> {
        let mut ini = Ini::new();
        ini.load(path).map_err(|e| ConfigError::ReadError {
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

/// Look up config file in standard locations.
pub fn find_config() -> Option<PathBuf> {
    // 1. current directory
    let local = PathBuf::from("justfolders.ini");
    if local.exists() {
        return Some(local);
    }

    // 2. user config dir (~/.config/justfolders/config.ini)
    if let Some(config_dir) = dirs::config_dir() {
        let user = config_dir.join("justfolders").join("config.ini");
        if user.exists() {
            return Some(user);
        }
    }

    // 3. system level
    let system = PathBuf::from("/etc/justfolders/config.ini");
    if system.exists() {
        return Some(system);
    }

    None
}
