use crate::core::config_generator::{generate_config_content, save_config, ConfigGeneratorParams};
use crate::core::parser::InitArgs;
use std::path::PathBuf;

pub fn run(args: InitArgs, config_path: &PathBuf) {
    let (Some(src), Some(dest)) = (args.src, args.dest) else {
        eprintln!("error: --src and --dest are required for init");
        std::process::exit(1);
    };

    let params = ConfigGeneratorParams {
        src,
        dest,
        port: args.ssh_port,
        id_rsa: args.id_rsa,
        disable_logs: args.disable_logs,
        log_dir: args.log_dir,
        exclude_from: args.exclude_from,
        rsync_flags: None,
    };

    let content = generate_config_content(&params);

    if args.dry_run {
        println!("{}", content);
        return;
    }

    if config_path.exists() {
        eprintln!(
            "error: config file already exists at {}, delete it first or edit it directly",
            config_path.display()
        );
        std::process::exit(1);
    }

    match save_config(&content, config_path) {
        Ok(_) => println!("config saved to {}", config_path.display()),
        Err(e) => {
            eprintln!("error: {}", e);
            std::process::exit(1);
        }
    }
}