use justfolders::core::backup::{Backup, BackupError};
use justfolders::core::parser::CliParams;
use std::fs;
// use std::path::{Path, PathBuf};
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

// fn file_name_string(path: &Path) -> String {
//     path.file_name()
//         .and_then(|f| f.to_str())
//         .unwrap_or_default()
//         .to_string()
// }

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
