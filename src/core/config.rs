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
    /// Optional SMTP notification settings. `None` means notifications are disabled.
    pub notify: Option<SmtpConfig>,
}

/// TLS mode used when connecting to the SMTP server.
#[derive(Debug, Clone, PartialEq)]
pub enum SmtpTls {
    /// STARTTLS on port 587 (recommended for most providers).
    StartTls,
    /// Implicit TLS on port 465.
    Tls,
    /// No encryption — for local relays / development only.
    None,
}

impl SmtpTls {
    /// Parses a TLS mode string from the config file.
    ///
    /// Accepted values (case-insensitive): `"starttls"`, `"tls"`, `"none"`.
    /// Returns [`ConfigError::InvalidValue`] for any other input.
    fn parse(s: &str) -> Result<Self, ConfigError> {
        match s.trim().to_lowercase().as_str() {
            "starttls" => Ok(Self::StartTls),
            "tls" => Ok(Self::Tls),
            "none" => Ok(Self::None),
            other => Err(ConfigError::InvalidValue {
                field: "tls".to_string(),
                reason: format!("expected 'starttls', 'tls', or 'none'; got '{other}'"),
            }),
        }
    }
}

/// SMTP settings loaded from the `[notify]` section of the config file.
#[derive(Debug, Clone)]
pub struct SmtpConfig {
    pub smtp_host: String,
    pub smtp_port: u16,
    pub smtp_user: String,
    pub smtp_password: String,
    pub from: String,
    pub to: String,
    /// TLS mode. Defaults to [`SmtpTls::StartTls`].
    pub tls: SmtpTls,
    /// Send a notification even when the backup succeeds. Defaults to `false`.
    pub on_success: bool,
}

impl SmtpConfig {
    /// Loads SMTP settings from the `[notify]` INI section.
    ///
    /// Returns `Ok(None)` when the section is absent (notifications disabled).
    /// `from` and `to` addresses are validated as parseable `Mailbox` values at
    /// load time so callers can rely on them being well-formed without re-checking.
    fn load(ini: &Ini) -> Result<Option<Self>, ConfigError> {
        // If the section is absent entirely, notifications are disabled.
        let smtp_host = match ini.get("notify", "smtp_host") {
            Some(v) => v,
            None => return Ok(None),
        };

        let smtp_port = ini
            .getuint("notify", "smtp_port")
            .map_err(|e| ConfigError::InvalidValue {
                field: "smtp_port".to_string(),
                reason: e,
            })?
            .unwrap_or(587) as u16;

        let smtp_user = ini.get("notify", "smtp_user").unwrap_or_default();
        let smtp_password = ini.get("notify", "smtp_password").unwrap_or_default();

        let from = ini
            .get("notify", "from")
            .ok_or_else(|| ConfigError::MissingField("notify.from".to_string()))?;

        // Validate the from address is a parseable email address at config load time
        // so the user gets a clean error rather than a panic during send.
        from.parse::<lettre::message::Mailbox>()
            .map_err(|e| ConfigError::InvalidValue {
                field: "notify.from".to_string(),
                reason: e.to_string(),
            })?;

        let to = ini
            .get("notify", "to")
            .ok_or_else(|| ConfigError::MissingField("notify.to".to_string()))?;

        // Same validation for the to address.
        to.parse::<lettre::message::Mailbox>()
            .map_err(|e| ConfigError::InvalidValue {
                field: "notify.to".to_string(),
                reason: e.to_string(),
            })?;

        let tls = match ini.get("notify", "tls") {
            Some(v) => SmtpTls::parse(&v)?,
            None => SmtpTls::StartTls,
        };

        let on_success = ini
            .getbool("notify", "on_success")
            .map_err(|e| ConfigError::InvalidValue {
                field: "on_success".to_string(),
                reason: e,
            })?
            .unwrap_or(false);

        Ok(Some(SmtpConfig {
            smtp_host,
            smtp_port,
            smtp_user,
            smtp_password,
            from,
            to,
            tls,
            on_success,
        }))
    }
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
    pub keep_within_days: Option<u32>,
}

impl RetentionPolicy {
    /// Returns `true` when no retention rules are configured (all fields are
    /// `None` or set to `0`, which is treated as disabled).
    pub fn is_empty(&self) -> bool {
        let count_rules = [
            self.keep_hourly,
            self.keep_daily,
            self.keep_weekly,
            self.keep_monthly,
            self.keep_yearly,
        ];
        let within_rules = [
            self.keep_within_days,
        ];
        count_rules.iter().chain(within_rules.iter()).all(|v| v.map_or(true, |n| n == 0))
    }

    /// Loads retention policy settings from the `[policy]` INI section.
    ///
    /// Missing keys are mapped to `None` (rule disabled).  A value of `0` is
    /// stored as `Some(0)` and treated as disabled by [`RetentionPolicy::is_empty`]
    /// and the individual rule guards in the retention engine.
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
            keep_within_days: get_uint("keep_within_days")?,
        })
    }
}

/// Returns `ConfigError::InvalidValue` if `value` contains a single-quote character.
///
/// Single quotes are used to delimit paths inside remote shell commands (SSH, rsync `-e`).
/// A single quote inside the path closes the shell quoting prematurely and allows
/// arbitrary command injection.  Reject such paths at config load time rather than
/// silently misquoting them later.
fn reject_single_quote(field: &str, value: &str) -> Result<(), ConfigError> {
    if value.contains('\'') {
        Err(ConfigError::InvalidValue {
            field: field.to_string(),
            reason: "path must not contain a single-quote character (') \
                     because it would break remote shell quoting"
                .to_string(),
        })
    } else {
        Ok(())
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

        // A single-quote in any path that ends up inside a remote shell command
        // (SSH, rsync -e wrapper) breaks the quoting and enables shell injection.
        // Reject such paths at load time rather than silently misquoting them later.
        if src.is_remote() {
            reject_single_quote("src", src.path_str())?;
        }
        if dest.is_remote() {
            reject_single_quote("dest", dest.path_str())?;
        }
        if let Some(ref key) = id_rsa {
            // id_rsa is embedded as -i '<path>' in the rsync -e shell string.
            let key_str = key.to_string_lossy();
            reject_single_quote("id_rsa", &key_str)?;
        }

        let policy = RetentionPolicy::load(&ini)?;
        let notify = SmtpConfig::load(&ini)?;

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
            notify,
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

/// Renders a single `[policy]` config line.
///
/// When `value` is `Some(n)` the line is active: `key = n`.
/// When `value` is `None` the line is emitted as a comment placeholder so the
/// user can see the available option and enable it by uncommenting: `# key = 0`.
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
    let keep_within_days = optional_policy_line("keep_within_days", p.keep_within_days);

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

        # Keep every snapshot created within the last N days (0 = disabled)
        {keep_within_days}

        [notify]
        # Send email notifications on backup failure (and optionally success).
        # Remove the '#' from the lines below and fill in your SMTP details to enable.
        #
        # smtp_host     = smtp.example.com
        # smtp_port     = 587
        # smtp_user     = alerts@example.com
        # smtp_password = secret
        # from          = justfolders@example.com
        # to            = admin@example.com
        # tls           = starttls   # starttls (default) | tls | none
        # on_success    = false      # set to true to also notify on successful backups
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

    // Restrict permissions so SMTP credentials stored in the file are not
    // world-readable.  We intentionally ignore failures here (e.g. on
    // non-Unix platforms or read-only mounts) — a warning is enough.
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        if let Err(e) = std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o600)) {
            eprintln!(
                "warning: could not set config file permissions to 0600: {e}"
            );
        }
    }

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
