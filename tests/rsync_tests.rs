// use justfolders::core::backup::Backup;
// use justfolders::core::parser::CliParams;
// use justfolders::core::rsync::{Rsync, RsyncError};
// use serial_test::serial;
// use std::fs;
// use std::os::unix::fs::PermissionsExt;
// use std::path::Path;
// use tempfile::TempDir;

// fn make_params(src: &str, dest: &str) -> CliParams {
//     CliParams {
//         src: Some(src.to_string()),
//         dest: Some(dest.to_string()),
//         ssh_port: 2222,
//         disable_logs: false,
//         log_dir: None,
//         id_rsa: None,
//         rsync_get_flags: false,
//         rsync_set_flags: "-a --delete".to_string(),
//         rsync_append_flags: Some("-vv".to_string()),
//         exclude_from: Some("/tmp/excludes.txt".to_string()),
//     }
// }

// fn write_fake_rsync(bin_dir: &TempDir, args_output: &str, exit_code: i32) {
//     let script = format!(
//         "#!/bin/sh\nprintf '%s' \"$@\" > \"{}\"\nexit {}\n",
//         args_output, exit_code
//     );
//     let script_path = bin_dir.path().join("rsync");
//     fs::write(&script_path, script).expect("write fake rsync script");
//     let mut perms = fs::metadata(&script_path).expect("script metadata").permissions();
//     perms.set_mode(0o755);
//     fs::set_permissions(&script_path, perms).expect("set script executable");
// }

// fn read_args(file: &str) -> String {
//     fs::read_to_string(file).expect("read captured args")
// }

// struct PathGuard {
//     old_path: String,
// }

// impl Drop for PathGuard {
//     fn drop(&mut self) {
//         // SAFETY: test-only process env mutation restored in Drop.
//         unsafe { std::env::set_var("PATH", &self.old_path) };
//     }
// }

// fn prepend_to_path(path: &Path) -> PathGuard {
//     let old_path = std::env::var("PATH").unwrap_or_default();
//     let new_path = format!("{}:{}", path.display(), old_path);
//     // SAFETY: tests are serialized with #[serial], so mutating PATH is safe here.
//     unsafe { std::env::set_var("PATH", new_path) };
//     PathGuard { old_path }
// }

// #[test]
// #[serial]
// fn rsync_run_success_with_fake_binary() {
//     let workspace = TempDir::new().expect("create temp workspace");
//     let bin_dir = TempDir::new().expect("create temp bin dir");
//     let args_file = workspace.path().join("args.txt");
//     let args_file_str = args_file.display().to_string();
//     write_fake_rsync(&bin_dir, &args_file_str, 0);
//     let _path_guard = prepend_to_path(bin_dir.path());

//     let dest = workspace.path().join("dest");
//     fs::create_dir_all(&dest).expect("create dest");
//     fs::write(dest.join("backup.marker"), "").expect("create marker");
//     let params = make_params("/tmp/src", &dest.display().to_string());
//     let backup = Backup::new(&params);

//     let mut rsync = Rsync::new(&params, &backup).expect("rsync command should build");
//     rsync.run().expect("fake rsync exits 0");

//     let args = read_args(&args_file_str);
//     assert!(args.contains("--delete"));
//     assert!(args.contains("--exclude-from"));
//     assert!(args.contains("--log-file"));
//     assert!(args.contains("/tmp/src/"));
//     assert!(args.contains("tmp"));
// }

// #[test]
// #[serial]
// fn rsync_run_returns_non_zero_error() {
//     let workspace = TempDir::new().expect("create temp workspace");
//     let bin_dir = TempDir::new().expect("create temp bin dir");
//     let args_file = workspace.path().join("args.txt");
//     let args_file_str = args_file.display().to_string();
//     write_fake_rsync(&bin_dir, &args_file_str, 7);
//     let _path_guard = prepend_to_path(bin_dir.path());

//     let dest = workspace.path().join("dest");
//     fs::create_dir_all(&dest).expect("create dest");
//     fs::write(dest.join("backup.marker"), "").expect("create marker");
//     let params = make_params("/tmp/src", &dest.display().to_string());
//     let backup = Backup::new(&params);

//     let mut rsync = Rsync::new(&params, &backup).expect("rsync command should build");
//     let err = rsync.run().expect_err("fake rsync exits non-zero");
//     assert!(matches!(err, RsyncError::NonZeroStatus { .. }));
// }

// #[test]
// #[serial]
// fn rsync_builds_remote_ssh_and_link_dest_options() {
//     let workspace = TempDir::new().expect("create temp workspace");
//     let bin_dir = TempDir::new().expect("create temp bin dir");
//     let args_file = workspace.path().join("args.txt");
//     let args_file_str = args_file.display().to_string();
//     write_fake_rsync(&bin_dir, &args_file_str, 0);
//     let _path_guard = prepend_to_path(bin_dir.path());

//     let dest = workspace.path().join("dest");
//     fs::create_dir_all(&dest).expect("create dest");
//     fs::create_dir_all(dest.join("latest")).expect("create latest folder");
//     fs::write(dest.join("backup.marker"), "").expect("create marker");

//     let mut params = make_params("user@example.com:/srv/src", &dest.display().to_string());
//     params.id_rsa = Some(workspace.path().join("id_test"));
//     params.disable_logs = true;
//     let backup = Backup::new(&params);

//     let mut rsync = Rsync::new(&params, &backup).expect("rsync command should build");
//     rsync.run().expect("fake rsync exits 0");

//     let args = read_args(&args_file_str);
//     assert!(args.contains("--compress"));
//     assert!(args.contains("--link-dest"));
//     assert!(args.contains("ssh -p 2222"));
//     assert!(args.contains("-i"));
//     assert!(!args.contains("--log-file"));
// }
