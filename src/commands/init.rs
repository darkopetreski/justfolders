use crate::core::config_generator::{ConfigGeneratorParams, generate_config_content, save_config};
use crate::core::parser::InitArgs;
use thiserror::Error;

#[derive(Debug, Error)]
pub enum InitError {
    #[error("config file already exists at {0}, delete it first or edit it directly")]
    ConfigAlreadyExists(String),

    #[error(transparent)]
    SaveError(#[from] crate::core::config_generator::ConfigGeneratorError),
}

pub fn run(args: InitArgs) -> Result<(), InitError> {
    let params = ConfigGeneratorParams {
        src: args.src,
        dest: args.dest,
        port: args.ssh_port,
        id_rsa: args.id_rsa,
        disable_logs: args.disable_logs,
        log_dir: args.log_dir,
        exclude_from: args.exclude_from,
        rsync_flags: args.rsync_flags,
    };

    let content = generate_config_content(&params);

    if args.dry_run {
        println!("{}", content);
        return Ok(());
    }

    if args.config.exists() {
        return Err(InitError::ConfigAlreadyExists(
            args.config.display().to_string(),
        ));
    }

    save_config(&content, &args.config)?;
    println!("config saved to {}", args.config.display());
    Ok(())
}