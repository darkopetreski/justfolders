//! Integration tests for the remote `StorageBackend` checksum methods.
//!
//! These tests run end-to-end over a real SSH connection to `localhost`
//! and exercise the same surface as `tests/checksum_backend.rs` does for
//! the local backend.  When SSH-to-localhost is not available (CI hosts
//! without sshd, restricted sandboxes, …) every test skips itself with a
//! printed note instead of failing — this matches the project's existing
//! convention for backend-level tests that need real external tools.
//!
//! To run these in earnest, ensure:
//!   * `sshd` is running locally.
//!   * Your user's SSH key is in `~/.ssh/authorized_keys` (passwordless).
//!   * `ssh -o BatchMode=yes -o ConnectTimeout=2 localhost true` succeeds.

use justfolders::core::checksum::{HashedEntry, NeedsHashing};
use justfolders::core::config::{Config, RetentionPolicy};
use justfolders::core::storage_backend::{from_config, StorageBackend};
use std::collections::HashSet;
use std::fs;
use std::path::Path;
use std::process::{Command, Stdio};
use tempfile::TempDir;

// ── Skip-gate ───────────────────────────────────────────────────────────────

/// Try a no-op SSH session to localhost.  Returns `true` only when we can
/// connect, authenticate non-interactively, and run a trivial command.
/// Cached per process so the probe runs at most once.
fn ssh_to_localhost_available() -> bool {
    use std::sync::OnceLock;
    static AVAILABLE: OnceLock<bool> = OnceLock::new();
    *AVAILABLE.get_or_init(|| {
        Command::new("ssh")
            .args([
                "-o",
                "BatchMode=yes",
                "-o",
                "ConnectTimeout=2",
                "-o",
                "StrictHostKeyChecking=accept-new",
                "localhost",
                "true",
            ])
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status()
            .map(|s| s.success())
            .unwrap_or(false)
    })
}

// ── Helpers ─────────────────────────────────────────────────────────────────

/// Build a `Config` whose `dest` is `localhost:<dest_dir>` so `from_config`
/// returns a `RemoteBackend` we can drive through the public trait API.
fn remote_localhost_config(dest_dir: &Path) -> Config {
    let remote = format!("localhost:{}", dest_dir.display());
    Config {
        src: "/tmp/src".parse().unwrap(),
        dest: remote.parse().expect("parse remote location"),
        port: 22,
        id_rsa: None,
        disable_logs: true,
        log_dir: None,
        exclude_from: None,
        rsync_flags: None,
        bwlimit: None,
        policy: RetentionPolicy::default(),
        notify: None,
        strict: false,
        checksum: true,
    }
}

fn backend_for(dest_dir: &Path) -> Box<dyn StorageBackend> {
    from_config(&remote_localhost_config(dest_dir))
}

fn make_snapshot_dir(dest: &Path, name: &str) -> std::path::PathBuf {
    let snap = dest.join(name);
    fs::create_dir(&snap).expect("create snapshot dir");
    snap
}

const SHA256_OF_ABC: &str = "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad";

// ── list_files_with_inodes ──────────────────────────────────────────────────

#[test]
fn remote_list_files_with_inodes_returns_relative_paths() {
    if !ssh_to_localhost_available() {
        eprintln!("skipping: SSH to localhost not available");
        return;
    }

    let dest = TempDir::new().unwrap();
    let snap_name = "2026-05-22-100000";
    let snap = make_snapshot_dir(dest.path(), snap_name);
    fs::create_dir_all(snap.join("etc")).unwrap();
    fs::write(snap.join("etc/hosts"), b"x").unwrap();
    fs::write(snap.join("toplevel.txt"), b"y").unwrap();

    let backend = backend_for(dest.path());
    let listing = backend.list_files_with_inodes(snap_name).unwrap();
    let paths: HashSet<String> = listing.iter().map(|(_, p)| p.clone()).collect();
    assert_eq!(
        paths,
        HashSet::from(["etc/hosts".to_string(), "toplevel.txt".to_string()])
    );
}

#[test]
fn remote_list_files_with_inodes_empty_snapshot() {
    if !ssh_to_localhost_available() {
        eprintln!("skipping: SSH to localhost not available");
        return;
    }

    let dest = TempDir::new().unwrap();
    let snap_name = "2026-05-22-100000";
    make_snapshot_dir(dest.path(), snap_name);

    let backend = backend_for(dest.path());
    let listing = backend.list_files_with_inodes(snap_name).unwrap();
    assert!(listing.is_empty());
}

// ── hash_files ──────────────────────────────────────────────────────────────

#[test]
fn remote_hash_files_returns_known_sha256_values() {
    if !ssh_to_localhost_available() {
        eprintln!("skipping: SSH to localhost not available");
        return;
    }

    let dest = TempDir::new().unwrap();
    let snap_name = "2026-05-22-100000";
    let snap = make_snapshot_dir(dest.path(), snap_name);
    fs::write(snap.join("abc.txt"), b"abc").unwrap();

    let backend = backend_for(dest.path());
    let needs = vec![NeedsHashing {
        rel_path: "abc.txt".to_string(),
        inode: 1234,
    }];
    let hashed: Vec<HashedEntry> = backend.hash_files(snap_name, &needs).unwrap();
    assert_eq!(hashed.len(), 1);
    assert_eq!(hashed[0].rel_path, "abc.txt");
    assert_eq!(hashed[0].inode, 1234, "inode must be carried through unchanged");
    assert_eq!(hashed[0].hash, SHA256_OF_ABC);
}

#[test]
fn remote_hash_files_empty_input_skips_subprocess() {
    // Even without SSH this must not invoke ssh at all — the early-return
    // path exists precisely to make `if checksum_step::run` fast when a
    // snapshot has no new files.
    let dest = TempDir::new().unwrap();
    let snap_name = "2026-05-22-100000";
    make_snapshot_dir(dest.path(), snap_name);

    let backend = backend_for(dest.path());
    let result = backend.hash_files(snap_name, &[]).unwrap();
    assert!(result.is_empty());
}

#[test]
fn remote_hash_files_handles_paths_with_spaces() {
    if !ssh_to_localhost_available() {
        eprintln!("skipping: SSH to localhost not available");
        return;
    }

    let dest = TempDir::new().unwrap();
    let snap_name = "2026-05-22-100000";
    let snap = make_snapshot_dir(dest.path(), snap_name);
    fs::create_dir_all(snap.join("dir with spaces")).unwrap();
    fs::write(snap.join("dir with spaces/x.txt"), b"abc").unwrap();

    let backend = backend_for(dest.path());
    let needs = vec![NeedsHashing {
        rel_path: "dir with spaces/x.txt".to_string(),
        inode: 42,
    }];
    let hashed = backend.hash_files(snap_name, &needs).unwrap();
    assert_eq!(hashed[0].hash, SHA256_OF_ABC);
    assert_eq!(hashed[0].inode, 42);
}

// ── checksums.db (read/write) ───────────────────────────────────────────────

#[test]
fn remote_read_checksum_db_returns_none_when_missing() {
    if !ssh_to_localhost_available() {
        eprintln!("skipping: SSH to localhost not available");
        return;
    }

    let dest = TempDir::new().unwrap();
    let backend = backend_for(dest.path());
    assert!(backend.read_checksum_db().unwrap().is_none());
}

#[test]
fn remote_write_then_read_checksum_db_roundtrips() {
    if !ssh_to_localhost_available() {
        eprintln!("skipping: SSH to localhost not available");
        return;
    }

    let dest = TempDir::new().unwrap();
    let backend = backend_for(dest.path());

    let content = format!(
        "# justfolders-checksum-db v1\n1234\t{SHA256_OF_ABC}\tetc/hosts\n"
    );
    backend.write_checksum_db(&content).unwrap();

    let read_back = backend.read_checksum_db().unwrap().unwrap();
    assert_eq!(read_back, content);
}

#[test]
fn remote_write_checksum_db_is_atomic() {
    if !ssh_to_localhost_available() {
        eprintln!("skipping: SSH to localhost not available");
        return;
    }

    let dest = TempDir::new().unwrap();
    let backend = backend_for(dest.path());

    backend.write_checksum_db("first\n").unwrap();
    backend.write_checksum_db("second\n").unwrap();

    let content = backend.read_checksum_db().unwrap().unwrap();
    assert_eq!(content, "second\n");
    assert!(
        !dest.path().join("checksums.db.tmp").exists(),
        "no .tmp sibling should be left on success"
    );
}

// ── per-snapshot sha256sum (read/write) ─────────────────────────────────────

#[test]
fn remote_read_snapshot_sha256_returns_none_when_missing() {
    if !ssh_to_localhost_available() {
        eprintln!("skipping: SSH to localhost not available");
        return;
    }

    let dest = TempDir::new().unwrap();
    let snap_name = "2026-05-22-100000";
    make_snapshot_dir(dest.path(), snap_name);

    let backend = backend_for(dest.path());
    assert!(backend.read_snapshot_sha256(snap_name).unwrap().is_none());
}

#[test]
fn remote_write_then_read_snapshot_sha256_roundtrips() {
    if !ssh_to_localhost_available() {
        eprintln!("skipping: SSH to localhost not available");
        return;
    }

    let dest = TempDir::new().unwrap();
    let snap_name = "2026-05-22-100000";
    let snap = make_snapshot_dir(dest.path(), snap_name);
    let backend = backend_for(dest.path());

    let content = format!("{SHA256_OF_ABC}  payload.txt\n");
    backend.write_snapshot_sha256(snap_name, &content).unwrap();

    let read_back = backend.read_snapshot_sha256(snap_name).unwrap().unwrap();
    assert_eq!(read_back, content);
    assert!(!snap.join("sha256sum.tmp").exists());
}

#[test]
fn remote_write_snapshot_sha256_rejects_bad_snapshot_name() {
    if !ssh_to_localhost_available() {
        eprintln!("skipping: SSH to localhost not available");
        return;
    }

    let dest = TempDir::new().unwrap();
    let backend = backend_for(dest.path());

    let err = backend
        .write_snapshot_sha256("../etc", "x")
        .expect_err("path traversal must be rejected");
    assert!(err.to_string().contains("invalid snapshot name"));
}

// ── End-to-end interop with the standard sha256sum -c ───────────────────────

#[test]
fn remote_written_sha256sum_is_check_compatible() {
    if !ssh_to_localhost_available() {
        eprintln!("skipping: SSH to localhost not available");
        return;
    }

    // Write the snapshot file via the *remote* backend, then read it back
    // and feed it to the stock `sha256sum -c` binary locally — same file,
    // same format, regardless of which side did the writing.
    let dest = TempDir::new().unwrap();
    let snap_name = "2026-05-22-100000";
    let snap = make_snapshot_dir(dest.path(), snap_name);
    fs::write(snap.join("payload.txt"), b"abc").unwrap();

    let backend = backend_for(dest.path());
    let content = justfolders::core::checksum::format_sha256sum_file(&[HashedEntry {
        rel_path: "payload.txt".to_string(),
        inode: 0,
        hash: SHA256_OF_ABC.to_string(),
    }]);
    backend.write_snapshot_sha256(snap_name, &content).unwrap();

    let status = Command::new("sha256sum")
        .arg("-c")
        .arg("sha256sum")
        .current_dir(&snap)
        .status()
        .expect("run sha256sum -c");
    assert!(status.success());
}
