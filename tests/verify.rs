//! Integration tests for the `justfolders verify` command.
//!
//! These exercise the full pipeline: load config → pick backend → run
//! `sha256sum -c` → translate the exit code into a [`VerifyError`] or
//! success.  We construct snapshot directories by hand and call the
//! command's `run` function directly so the tests stay fast and require
//! no rsync.

use justfolders::commands::verify::{run as verify_run, VerifyError};
use justfolders::core::checksum::{format_sha256sum_file, HashedEntry};
use justfolders::core::parser::VerifyArgs;
use std::fs;
use std::path::Path;
use tempfile::TempDir;

// ── Helpers ─────────────────────────────────────────────────────────────────

/// Writes a minimal config pointing `dest` at a local directory.  Returns
/// the path to the config file so `VerifyArgs::config` can point at it.
fn write_local_config(tmp: &TempDir, dest_dir: &Path) -> std::path::PathBuf {
    let config_path = tmp.path().join("backup.ini");
    fs::write(
        &config_path,
        format!(
            "[default]\nsrc = /tmp/src\ndest = {}\n",
            dest_dir.display()
        ),
    )
    .unwrap();
    config_path
}

/// Sets up `<dest>/<name>/` containing one file `payload.txt = "abc"`,
/// plus a matching `sha256sum` file generated through our own formatter.
/// Returns the snapshot path so the test can tamper with it if needed.
fn snapshot_with_payload(dest: &Path, name: &str) -> std::path::PathBuf {
    let snap = dest.join(name);
    fs::create_dir(&snap).unwrap();
    fs::write(snap.join("payload.txt"), b"abc").unwrap();

    let sha = format_sha256sum_file(&[HashedEntry {
        rel_path: "payload.txt".to_string(),
        inode: 0,
        hash: "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad".to_string(),
    }]);
    fs::write(snap.join("sha256sum"), sha).unwrap();
    snap
}

fn have_binary(name: &str) -> bool {
    std::process::Command::new("sh")
        .arg("-c")
        .arg(format!("command -v {name}"))
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status()
        .map(|s| s.success())
        .unwrap_or(false)
}

// ── Tests ───────────────────────────────────────────────────────────────────

#[test]
fn verify_clean_snapshot_returns_ok() {
    if !have_binary("sha256sum") {
        eprintln!("skipping: sha256sum not available");
        return;
    }

    let tmp = TempDir::new().unwrap();
    let dest = TempDir::new().unwrap();
    let snap_name = "2026-05-22-100000";
    snapshot_with_payload(dest.path(), snap_name);
    let config = write_local_config(&tmp, dest.path());

    verify_run(VerifyArgs {
        config,
        snapshot: snap_name.to_string(),
    })
    .expect("clean snapshot must verify");
}

#[test]
fn verify_tampered_snapshot_reports_mismatch() {
    if !have_binary("sha256sum") {
        eprintln!("skipping: sha256sum not available");
        return;
    }

    let tmp = TempDir::new().unwrap();
    let dest = TempDir::new().unwrap();
    let snap_name = "2026-05-22-100000";
    let snap = snapshot_with_payload(dest.path(), snap_name);
    // Modify the file *after* the sha256sum was written → drift.
    fs::write(snap.join("payload.txt"), b"tampered").unwrap();

    let config = write_local_config(&tmp, dest.path());
    let err = verify_run(VerifyArgs {
        config,
        snapshot: snap_name.to_string(),
    })
    .expect_err("tampered snapshot must fail verification");

    match err {
        VerifyError::Mismatch(name) => assert_eq!(name, snap_name),
        other => panic!("expected Mismatch, got {other:?}"),
    }
}

#[test]
fn verify_snapshot_without_checksum_file_is_handled_gracefully() {
    let tmp = TempDir::new().unwrap();
    let dest = TempDir::new().unwrap();
    let snap_name = "2026-05-22-100000";
    let snap = dest.path().join(snap_name);
    fs::create_dir(&snap).unwrap();
    fs::write(snap.join("payload.txt"), b"abc").unwrap();
    // Intentionally no sha256sum file.

    let config = write_local_config(&tmp, dest.path());
    let err = verify_run(VerifyArgs {
        config,
        snapshot: snap_name.to_string(),
    })
    .expect_err("missing sha256sum should produce a clean error");

    match err {
        VerifyError::NoChecksumFile(name) => assert_eq!(name, snap_name),
        other => panic!("expected NoChecksumFile, got {other:?}"),
    }
}

#[test]
fn verify_latest_resolves_through_the_symlink() {
    if !have_binary("sha256sum") {
        eprintln!("skipping: sha256sum not available");
        return;
    }

    let tmp = TempDir::new().unwrap();
    let dest = TempDir::new().unwrap();
    let snap_name = "2026-05-22-100000";
    snapshot_with_payload(dest.path(), snap_name);

    // Manually establish the `latest` symlink, mirroring what `do_backup`
    // does at the end of every run.
    std::os::unix::fs::symlink(dest.path().join(snap_name), dest.path().join("latest"))
        .unwrap();

    let config = write_local_config(&tmp, dest.path());
    verify_run(VerifyArgs {
        config,
        snapshot: "latest".to_string(),
    })
    .expect("verify latest must follow the symlink");
}

#[test]
fn verify_rejects_invalid_snapshot_names() {
    let tmp = TempDir::new().unwrap();
    let dest = TempDir::new().unwrap();
    let config = write_local_config(&tmp, dest.path());

    let err = verify_run(VerifyArgs {
        config,
        snapshot: "../etc/passwd".to_string(),
    })
    .expect_err("path traversal must be rejected");

    // The InvalidSnapshotName error is wrapped in VerifyError::Backend.
    let msg = err.to_string();
    assert!(
        msg.contains("invalid snapshot name"),
        "expected snapshot-name rejection, got: {msg}"
    );
}

#[test]
fn verify_rejects_unknown_snapshot() {
    let tmp = TempDir::new().unwrap();
    let dest = TempDir::new().unwrap();
    let config = write_local_config(&tmp, dest.path());

    // Well-formed timestamp, but the directory does not exist.  The local
    // backend currently maps this to NoChecksumFile because the existence
    // probe runs before sha256sum -c — that's a reasonable user-facing
    // message.
    let err = verify_run(VerifyArgs {
        config,
        snapshot: "2099-01-01-000000".to_string(),
    })
    .expect_err("nonexistent snapshot must produce an error");

    assert!(matches!(err, VerifyError::NoChecksumFile(_)));
}
