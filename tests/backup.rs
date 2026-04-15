use justfolders::core::config::{Config, RetentionPolicy};
use justfolders::core::storage_backend::{StorageBackendError, from_config};
use std::fs;
use tempfile::TempDir;

fn make_local_config(dest: &str) -> Config {
    Config {
        src: "/tmp/src".parse().unwrap(),
        dest: dest.parse().unwrap(),
        port: 22,
        id_rsa: None,
        disable_logs: true,
        log_dir: None,
        exclude_from: None,
        rsync_flags: None,
        policy: RetentionPolicy::default(),
    }
}

#[test]
fn list_snapshots_empty() {
    let dest_dir = TempDir::new().expect("create temp dir");
    let config = make_local_config(&dest_dir.path().display().to_string());
    let backend = from_config(&config);

    let snapshots = backend.list_snapshots().expect("list snapshots");
    assert!(snapshots.is_empty());
}

#[test]
fn list_snapshots_returns_only_timestamped_dirs() {
    let dest_dir = TempDir::new().expect("create temp dir");
    let base = dest_dir.path();

    fs::create_dir(base.join("2025-01-01-120000")).unwrap();
    fs::create_dir(base.join("2025-06-15-083045")).unwrap();
    fs::create_dir(base.join("latest")).unwrap();
    fs::write(base.join("backup.marker"), "").unwrap();

    let config = make_local_config(&base.display().to_string());
    let backend = from_config(&config);

    let mut snapshots = backend.list_snapshots().expect("list snapshots");
    snapshots.sort();
    assert_eq!(snapshots, vec!["2025-01-01-120000", "2025-06-15-083045"]);
}

#[test]
fn delete_snapshot_removes_dir() {
    let dest_dir = TempDir::new().expect("create temp dir");
    let base = dest_dir.path();
    let snap = "2025-01-01-120000";

    fs::create_dir(base.join(snap)).unwrap();
    assert!(base.join(snap).exists());

    let config = make_local_config(&base.display().to_string());
    let backend = from_config(&config);

    backend.delete_snapshot(snap).expect("delete snapshot");
    assert!(!base.join(snap).exists());
}

#[test]
fn latest_symlink_is_created_and_updated() {
    let dest_dir = TempDir::new().expect("create temp dir");
    let base = dest_dir.path();

    let config = make_local_config(&base.display().to_string());
    let backend = from_config(&config);

    assert!(!backend.latest_exists().unwrap());

    let snap1 = "2025-01-01-120000";
    let snap2 = "2025-06-15-083045";
    fs::create_dir(base.join(snap1)).unwrap();
    fs::create_dir(base.join(snap2)).unwrap();

    backend.update_latest_symlink(snap1).expect("create symlink");
    assert!(backend.latest_exists().unwrap());

    backend.update_latest_symlink(snap2).expect("update symlink");
    let target = fs::read_link(base.join("latest")).unwrap();
    assert_eq!(target, base.join(snap2));
}
