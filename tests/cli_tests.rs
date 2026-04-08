// use std::process::Command;
// use std::{fs, os::unix::fs::PermissionsExt};
// use tempfile::TempDir;

// #[test]
// fn rsync_get_flags_prints_and_exits_successfully() {
//     let output = Command::new(env!("CARGO_BIN_EXE_justfolders"))
//         .arg("--rsync-get-flags")
//         .output()
//         .expect("binary should start");

//     assert!(output.status.success(), "expected zero exit status");
//     let stdout = String::from_utf8_lossy(&output.stdout);
//     assert!(stdout.contains("--numeric-ids"));
//     assert!(stdout.contains("--delete"));
// }

// fn write_fake_rsync(bin_dir: &TempDir) {
//     let script = r#"#!/bin/sh
// for last; do :; done
// case "${FAKE_RSYNC_MODE:-success}" in
//   fail)
//     exit 7
//     ;;
//   success)
//     mkdir -p "$last"
//     echo payload > "$last/from-rsync.txt"
//     exit 0
//     ;;
//   no_tmp)
//     exit 0
//     ;;
// esac
// "#;
//     let script_path = bin_dir.path().join("rsync");
//     fs::write(&script_path, script).expect("write fake rsync script");
//     let mut perms = fs::metadata(&script_path).expect("script metadata").permissions();
//     perms.set_mode(0o755);
//     fs::set_permissions(&script_path, perms).expect("set executable bit");
// }

// #[test]
// fn main_fails_when_marker_missing() {
//     let temp = TempDir::new().expect("create temp dir");
//     let dest = temp.path().join("dest");
//     fs::create_dir_all(&dest).expect("create destination");

//     let output = Command::new(env!("CARGO_BIN_EXE_justfolders"))
//         .arg("/tmp/source")
//         .arg(dest.display().to_string())
//         .output()
//         .expect("binary should start");

//     assert!(!output.status.success(), "missing marker should fail");
//     let stderr = String::from_utf8_lossy(&output.stderr);
//     assert!(stderr.contains("missing backup.marker"));
// }

// #[test]
// fn main_fails_when_rsync_returns_non_zero() {
//     let temp = TempDir::new().expect("create temp dir");
//     let bin_dir = TempDir::new().expect("create temp bin");
//     write_fake_rsync(&bin_dir);

//     let dest = temp.path().join("dest");
//     fs::create_dir_all(&dest).expect("create destination");
//     fs::write(dest.join("backup.marker"), "").expect("create marker");

//     let path = std::env::var("PATH").unwrap_or_default();
//     let output = Command::new(env!("CARGO_BIN_EXE_justfolders"))
//         .arg("/tmp/source")
//         .arg(dest.display().to_string())
//         .env("PATH", format!("{}:{}", bin_dir.path().display(), path))
//         .env("FAKE_RSYNC_MODE", "fail")
//         .output()
//         .expect("binary should start");

//     assert!(!output.status.success(), "rsync failure should fail program");
//     let stderr = String::from_utf8_lossy(&output.stderr);
//     assert!(stderr.contains("command exited unsuccessfully"));
// }

// #[test]
// fn main_fails_when_rsync_does_not_create_tmp_folder() {
//     let temp = TempDir::new().expect("create temp dir");
//     let bin_dir = TempDir::new().expect("create temp bin");
//     write_fake_rsync(&bin_dir);

//     let dest = temp.path().join("dest");
//     fs::create_dir_all(&dest).expect("create destination");
//     fs::write(dest.join("backup.marker"), "").expect("create marker");

//     let path = std::env::var("PATH").unwrap_or_default();
//     let output = Command::new(env!("CARGO_BIN_EXE_justfolders"))
//         .arg("/tmp/source")
//         .arg(dest.display().to_string())
//         .env("PATH", format!("{}:{}", bin_dir.path().display(), path))
//         .env("FAKE_RSYNC_MODE", "no_tmp")
//         .output()
//         .expect("binary should start");

//     assert!(!output.status.success(), "missing tmp after rsync should fail");
//     let stderr = String::from_utf8_lossy(&output.stderr);
//     assert!(stderr.contains("in-progress folder missing"));
// }

// #[test]
// fn main_successfully_completes_backup_flow() {
//     let temp = TempDir::new().expect("create temp dir");
//     let bin_dir = TempDir::new().expect("create temp bin");
//     write_fake_rsync(&bin_dir);

//     let dest = temp.path().join("dest");
//     fs::create_dir_all(&dest).expect("create destination");
//     fs::write(dest.join("backup.marker"), "").expect("create marker");

//     let path = std::env::var("PATH").unwrap_or_default();
//     let output = Command::new(env!("CARGO_BIN_EXE_justfolders"))
//         .arg("/tmp/source")
//         .arg(dest.display().to_string())
//         .env("PATH", format!("{}:{}", bin_dir.path().display(), path))
//         .env("FAKE_RSYNC_MODE", "success")
//         .output()
//         .expect("binary should start");

//     assert!(output.status.success(), "full flow should succeed");
//     let latest = dest.join("latest");
//     assert!(latest.exists(), "latest symlink should be created");
//     let target = fs::read_link(latest).expect("latest should be symlink");
//     assert!(target.join("from-rsync.txt").exists());
// }
