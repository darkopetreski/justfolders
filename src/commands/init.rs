use crate::core::config::{
    ConfigGeneratorParams, RetentionPolicy, ensure_ini_extension, generate_config_content,
    save_config,
};
use crate::core::parser::InitArgs;
use thiserror::Error;

/// Errors that can occur when generating a config file.
#[derive(Debug, Error)]
pub enum InitError {
    #[error("config file already exists at {0}, delete it first or edit it directly")]
    ConfigAlreadyExists(String),

    #[error(transparent)]
    SaveError(#[from] crate::core::config::ConfigError),
}

impl From<InitArgs> for ConfigGeneratorParams {
    fn from(args: InitArgs) -> Self {
        ConfigGeneratorParams {
            src: args.src,
            dest: args.dest,
            port: args.ssh_port,
            id_rsa: args.id_rsa,
            disable_logs: args.disable_logs,
            log_dir: args.log_dir,
            exclude_from: args.exclude_from,
            rsync_flags: args.rsync_flags,
            policy: RetentionPolicy {
                keep_hourly: args.keep_hourly,
                keep_daily: args.keep_daily,
                keep_weekly: args.keep_weekly,
                keep_monthly: args.keep_monthly,
                keep_yearly: args.keep_yearly,
                keep_within_hourly: args.keep_within_hourly,
                keep_within_daily: args.keep_within_daily,
                keep_within_weekly: args.keep_within_weekly,
                keep_within_monthly: args.keep_within_monthly,
                keep_within_yearly: args.keep_within_yearly,
            },
        }
    }
}

/// Generates a config file from the provided CLI arguments and writes it to disk.
///
/// With `--dry-run` the rendered content is printed to stdout instead.
pub fn run(args: InitArgs) -> Result<(), InitError> {
    let config_path = ensure_ini_extension(&args.config);
    let dry_run = args.dry_run;
    let content = generate_config_content(&ConfigGeneratorParams::from(args));

    if dry_run {
        println!("{}", content);
        return Ok(());
    }

    if config_path.exists() {
        return Err(InitError::ConfigAlreadyExists(
            config_path.display().to_string(),
        ));
    }

    save_config(&content, &config_path)?;
    println!("Config created, run the backup now: justfolders backup --config {}", config_path.display());
    Ok(())
}
