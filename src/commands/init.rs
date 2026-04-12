use crate::core::config::{
    ConfigGeneratorParams, RetentionPolicy, ensure_ini_extension, generate_config_content,
    save_config,
};
use crate::core::parser::InitArgs;
use thiserror::Error;

#[derive(Debug, Error)]
pub enum InitError {
    #[error("config file already exists at {0}, delete it first or edit it directly")]
    ConfigAlreadyExists(String),

    #[error(transparent)]
    SaveError(#[from] crate::core::config::ConfigError),
}

pub fn run(args: InitArgs) -> Result<(), InitError> {
    let config_path = ensure_ini_extension(&args.config);

    let policy = RetentionPolicy {
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
    };

    let params = ConfigGeneratorParams {
        src: args.src,
        dest: args.dest,
        port: args.ssh_port,
        id_rsa: args.id_rsa,
        disable_logs: args.disable_logs,
        log_dir: args.log_dir,
        exclude_from: args.exclude_from,
        rsync_flags: args.rsync_flags,
        policy,
    };

    let content = generate_config_content(&params);

    if args.dry_run {
        println!("{}", content);
        return Ok(());
    }

    if config_path.exists() {
        return Err(InitError::ConfigAlreadyExists(
            args.config.display().to_string(),
        ));
    }

    save_config(&content, &config_path)?;
    println!("config saved to {}", config_path.display());
    Ok(())
}
