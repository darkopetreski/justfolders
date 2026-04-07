use justfolders::core::backup::{Backup, BackupError};
use justfolders::core::parser::CliParams;
use std::fs;
use std::path::{Path, PathBuf};
use tempfile::TempDir;

fn make_params(src: Option<String>, dest: Option<String>) -> CliParams {
    CliParams {
        src,
        dest,
        ssh_port: 22,
        disable_logs: false,
        log_dir: None,
        id_rsa: None,
        rsync_get_flags: false,
        rsync_set_flags: "-a".to_string(),
        rsync_append_flags: None,
        exclude_from: None,
    }
}

fn file_name_string(path: &Path) -> String {
    path.file_name()
        .and_then(|f| f.to_str())
        .unwrap_or_default()
        .to_string()
}

#[test]
fn backup_marker_checks() {
    let dest_dir = TempDir::new().expect("create temp dir");
    let params = make_params(
        Some("/tmp/src".to_string()),
        Some(dest_dir.path().display().to_string()),
    );
    let backup = Backup::new(&params);

    let err = backup.ensure_marker().expect_err("missing marker should fail");
    assert!(matches!(err, BackupError::MissingMarker { .. }));

    fs::write(dest_dir.path().join("backup.marker"), "").expect("create marker");
    backup.ensure_marker().expect("marker should be accepted");
}

#[test]
fn backup_folder_helpers_and_log_filename() {
    let dest_dir = TempDir::new().expect("create temp dir");
    let params = make_params(
        Some("/tmp/source".to_string()),
        Some(dest_dir.path().display().to_string()),
    );
    let backup = Backup::new(&params);

    assert_eq!(backup.in_progress_folder_name(), "tmp");
    assert_eq!(backup.get_latest_folder(), dest_dir.path().join("latest"));
    assert_eq!(backup.get_in_progress_folder(), dest_dir.path().join("tmp"));

    let ts_folder = backup.get_new_timestamp_folder();
    assert!(ts_folder.starts_with(dest_dir.path()));
    assert_eq!(file_name_string(&ts_folder).len(), 17);

    let default_log = backup.get_log_filename().expect("logs enabled by default");
    assert!(default_log.starts_with(dest_dir.path().join(".logs")));
    assert_eq!(file_name_string(&default_log).len(), 17);

    let custom_log_dir = TempDir::new().expect("create custom log dir");
    let custom_params = CliParams {
        log_dir: Some(custom_log_dir.path().to_path_buf()),
        ..make_params(
            Some("/tmp/source".to_string()),
            Some(dest_dir.path().display().to_string()),
        )
    };
    let custom_backup = Backup::new(&custom_params);
    let custom_log = custom_backup
        .get_log_filename()
        .expect("custom log path should be returned");
    assert!(custom_log.starts_with(custom_log_dir.path()));

    let disabled_log_params = CliParams {
        disable_logs: true,
        ..make_params(
            Some("/tmp/source".to_string()),
            Some(dest_dir.path().display().to_string()),
        )
    };
    let disabled_backup = Backup::new(&disabled_log_params);
    assert!(disabled_backup.get_log_filename().is_none());
}

#[test]
fn move_in_progress_errors_when_missing() {
    let dest_dir = TempDir::new().expect("create temp dir");
    let params = make_params(
        Some("/tmp/source".to_string()),
        Some(dest_dir.path().display().to_string()),
    );
    let backup = Backup::new(&params);
    let err = backup
        .move_in_progress_to_latest()
        .expect_err("missing tmp dir should fail");
    assert!(matches!(err, BackupError::MissingInProgressFolder));
}

#[test]
fn move_in_progress_creates_snapshot_and_latest_symlink() {
    let dest_dir = TempDir::new().expect("create temp dir");
    let params = make_params(
        Some("/tmp/source".to_string()),
        Some(dest_dir.path().display().to_string()),
    );
    let backup = Backup::new(&params);

    let tmp_dir = backup.get_in_progress_folder();
    fs::create_dir_all(&tmp_dir).expect("create tmp dir");
    fs::write(tmp_dir.join("data.txt"), "payload").expect("create tmp content");

    backup
        .move_in_progress_to_latest()
        .expect("move tmp to timestamp should succeed");

    assert!(!tmp_dir.exists(), "tmp dir should be moved away");
    let latest = backup.get_latest_folder();
    assert!(latest.exists(), "latest symlink should exist");

    let target = fs::read_link(&latest).expect("latest should be a symlink");
    assert!(target.exists(), "latest should point to real snapshot folder");

    let snapshot_file = target.join("data.txt");
    assert!(snapshot_file.exists(), "snapshot should keep original contents");
}

#[test]
fn move_in_progress_replaces_existing_latest_symlink() {
    let dest_dir = TempDir::new().expect("create temp dir");
    let params = make_params(
        Some("/tmp/source".to_string()),
        Some(dest_dir.path().display().to_string()),
    );
    let backup = Backup::new(&params);

    let old_snapshot = dest_dir.path().join("old-snapshot");
    fs::create_dir_all(&old_snapshot).expect("create old snapshot");
    std::os::unix::fs::symlink(&old_snapshot, backup.get_latest_folder())
        .expect("create latest symlink");

    let tmp_dir = backup.get_in_progress_folder();
    fs::create_dir_all(&tmp_dir).expect("create tmp dir");
    fs::write(tmp_dir.join("new.txt"), "payload").expect("create tmp file");

    backup
        .move_in_progress_to_latest()
        .expect("move should replace latest symlink");

    let latest_target = fs::read_link(backup.get_latest_folder()).expect("latest link should exist");
    assert_ne!(latest_target, old_snapshot);
    assert!(latest_target.join("new.txt").exists());
}

#[test]
fn parser_path_and_remote_helpers_work() {
    let local = make_params(
        Some("/home/user/src".to_string()),
        Some("/home/user/dest".to_string()),
    );
    assert!(!local.is_remote());
    assert_eq!(local.src_path(), PathBuf::from("/home/user/src/"));
    assert_eq!(local.dest_path(), PathBuf::from("/home/user/dest"));

    let remote_src = make_params(
        Some("user@example.com:/srv/data".to_string()),
        Some("/home/user/dest".to_string()),
    );
    assert!(remote_src.is_remote());

    let remote_dest = make_params(
        Some("/home/user/src".to_string()),
        Some("user@example.com:/srv/backup".to_string()),
    );
    assert!(remote_dest.is_remote());
}

#[test]
fn parser_handle_rsync_get_flags_noop_when_disabled() {
    let args = make_params(
        Some("/home/user/src".to_string()),
        Some("/home/user/dest".to_string()),
    );
    CliParams::handle_rsync_get_flags(&args);
}
