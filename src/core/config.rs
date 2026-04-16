use crate::core::location::Location;
use configparser::ini::Ini;
use std::path::{Path, PathBuf};
use thiserror::Error;

// ── Errors ─────────────────────────────────────────────────────────────────

/// Errors that can occur when loading or saving a config file.
#[derive(Debug, Error)]
pub enum ConfigError {
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

/// Parsed representation of a JustFolders configuration file.
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
    /// Maximum transfer bandwidth in KB/s. `None` means unlimited.
    pub bwlimit: Option<u32>,
    pub policy: RetentionPolicy,
}

/// Snapshot retention rules loaded from the `[policy]` section of the config file.
///
/// Each field controls how many snapshots of a given granularity to keep.
/// `None` means the rule is disabled.
#[derive(Debug, Default)]
pub struct RetentionPolicy {
    pub keep_hourly: Option<u32>,
    pub keep_daily: Option<u32>,
    pub keep_weekly: Option<u32>,
    pub keep_monthly: Option<u32>,
    pub keep_yearly: Option<u32>,
    pub keep_within_hourly: Option<u32>,
    pub keep_within_daily: Option<u32>,
    pub keep_within_weekly: Option<u32>,
    pub keep_within_monthly: Option<u32>,
    pub keep_within_yearly: Option<u32>,
}

impl RetentionPolicy {
    fn load(ini: &Ini) -> Result<Self, ConfigError> {
        let get_uint = |key: &str| -> Result<Option<u32>, ConfigError> {
            ini.getuint("policy", key)
                .map(|v| v.map(|n| n as u32))
                .map_err(|e| ConfigError::InvalidValue {
                    field: key.to_string(),
                    reason: e,
                })
        };

        Ok(RetentionPolicy {
            keep_hourly: get_uint("keep_hourly")?,
            keep_daily: get_uint("keep_daily")?,
            keep_weekly: get_uint("keep_weekly")?,
            keep_monthly: get_uint("keep_monthly")?,
            keep_yearly: get_uint("keep_yearly")?,
            keep_within_hourly: get_uint("keep_within_hourly")?,
            keep_within_daily: get_uint("keep_within_daily")?,
            keep_within_weekly: get_uint("keep_within_weekly")?,
            keep_within_monthly: get_uint("keep_within_monthly")?,
            keep_within_yearly: get_uint("keep_within_yearly")?,
        })
    }
}

impl Config {
    /// Load config from a path. If the path is just a name (no directory
    /// component), look for it in the current directory.
    pub fn load(path: &Path) -> Result<Self, ConfigError> {
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

        let bwlimit = ini
            .getuint("default", "bwlimit")
            .map_err(|e| ConfigError::InvalidValue {
                field: "bwlimit".to_string(),
                reason: e,
            })?
            .map(|v| v as u32);

        let disable_logs = ini
            .getbool("default", "disable_logs")
            .map_err(|e| ConfigError::InvalidValue {
                field: "disable_logs".to_string(),
                reason: e,
            })?
            .unwrap_or(false);

        let policy = RetentionPolicy::load(&ini)?;

        Ok(Config {
            src,
            dest,
            port,
            id_rsa,
            disable_logs,
            log_dir,
            exclude_from,
            rsync_flags,
            bwlimit,
            policy,
        })
    }
}

// ── Generator params ───────────────────────────────────────────────────────

/// Parameters used to render a new config file via [`generate_config_content`].
///
/// Populated from [`crate::core::parser::InitArgs`] via its `From` conversion.
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
    /// Maximum transfer bandwidth in KB/s. `None` means unlimited.
    pub bwlimit: Option<u32>,
    pub policy: RetentionPolicy,
}

// ── Generation ─────────────────────────────────────────────────────────────

fn optional_policy_line(key: &str, value: Option<u32>) -> String {
    match value {
        Some(v) => format!("{} = {}", key, v),
        None => format!("# {} = 0", key),
    }
}

/// Renders a complete `.ini` config file as a string from the given parameters.
///
/// Optional fields that are `None` are emitted as commented-out placeholders so
/// the user can see and enable them easily.
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

    let bwlimit = match params.bwlimit {
        Some(kb) => format!("bwlimit = {}", kb),
        None => "# bwlimit = 0".to_string(),
    };

    let p = &params.policy;
    let keep_hourly = optional_policy_line("keep_hourly", p.keep_hourly);
    let keep_daily = optional_policy_line("keep_daily", p.keep_daily);
    let keep_weekly = optional_policy_line("keep_weekly", p.keep_weekly);
    let keep_monthly = optional_policy_line("keep_monthly", p.keep_monthly);
    let keep_yearly = optional_policy_line("keep_yearly", p.keep_yearly);
    let keep_within_hourly = optional_policy_line("keep_within_hourly", p.keep_within_hourly);
    let keep_within_daily = optional_policy_line("keep_within_daily", p.keep_within_daily);
    let keep_within_weekly = optional_policy_line("keep_within_weekly", p.keep_within_weekly);
    let keep_within_monthly = optional_policy_line("keep_within_monthly", p.keep_within_monthly);
    let keep_within_yearly = optional_policy_line("keep_within_yearly", p.keep_within_yearly);

    let raw = format!(
        "
        # JustFolders configuration file
        # Edit this file to configure your backup job.
        # Lines starting with # are comments and are ignored.
        # Documentation: https://github.com/darkopetreski/justfolders

        [default]
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

        # Limit transfer bandwidth in KB/s (optional, 0 = unlimited)
        # Example: 1000 = ~1 MB/s, 10000 = ~10 MB/s
        {bwlimit}

        # Override default rsync flags (optional)
        # Default: -D --numeric-ids --links --hard-links --one-file-system
        #          --itemize-changes --times --recursive --perms --owner
        #          --group --stats --human-readable --delete
        rsync_flags = {rsync_flags}
        
        [policy]
        # Number of hourly snapshots to keep (0 = disabled)
        {keep_hourly}

        # Number of daily snapshots to keep (0 = disabled)
        {keep_daily}

        # Number of weekly snapshots to keep (0 = disabled)
        {keep_weekly}

        # Number of monthly snapshots to keep (0 = disabled)
        {keep_monthly}

        # Number of yearly snapshots to keep (0 = disabled)
        {keep_yearly}

        # Keep all snapshots within the last N hours (0 = disabled)
        {keep_within_hourly}

        # Keep all snapshots within the last N days (0 = disabled)
        {keep_within_daily}

        # Keep all snapshots within the last N weeks (0 = disabled)
        {keep_within_weekly}

        # Keep all snapshots within the last N months (0 = disabled)
        {keep_within_monthly}

        # Keep all snapshots within the last N years (0 = disabled)
        {keep_within_yearly}
        ",
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

/// Writes `content` to `path`, creating any missing parent directories first.
pub fn save_config(content: &str, path: &Path) -> Result<(), ConfigError> {
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
pub fn resolve_path(path: &Path) -> PathBuf {
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

/// Returns a copy of `path` with a `.ini` extension, adding it if absent.
pub fn ensure_ini_extension(path: &Path) -> PathBuf {
    match path.extension() {
        Some(ext) if ext == "ini" => path.to_path_buf(),
        _ => path.with_extension("ini"),
    }
}
