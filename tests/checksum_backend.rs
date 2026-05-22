//! Integration tests for the checksum/verification I/O surface on
//! [`crate::core::storage_backend::StorageBackend`].
//!
//! These exercise the real `find` and `sha256sum` binaries against a
//! local temp directory; they are skipped automatically (with a printed
//! note) if either binary is missing on the host.
//!
//! Step 4 will add a parallel test file for the remote SSH-backed
//! implementations.

use justfolders::core::checksum::{ChecksumDb, HashedEntry, NeedsHashing};
use justfolders::core::config::{Config, RetentionPolicy};
use justfolders::core::storage_backend::{from_config, StorageBackend};
use std::collections::HashSet;
use std::fs;
use std::os::unix::fs::MetadataExt;
use std::path::Path;
use tempfile::TempDir;

// ── Helpers ─────────────────────────────────────────────────────────────────

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
        bwlimit: None,
        policy: RetentionPolicy::default(),
        notify: None,
        strict: false,
    }
}

/// Build a `LocalBackend` rooted at `dest_dir`.  The trait object is what
/// the rest of the codebase actually works with, so we keep the tests at
/// that level.
fn backend_for(dest_dir: &Path) -> Box<dyn StorageBackend> {
    let config = make_local_config(&dest_dir.display().to_string());
    from_config(&config)
}

/// Returns `true` if the named binary is available on `$PATH`.  Used to
/// skip these tests gracefully on hosts without coreutils/findutils.
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

/// Sets up a fresh snapshot directory `<dest>/<name>/` and returns its path.
fn make_snapshot_dir(dest: &Path, name: &str) -> std::path::PathBuf {
    let snap = dest.join(name);
    fs::create_dir(&snap).expect("create snapshot dir");
    snap
}

/// SHA-256 of the byte string "abc".  A standard NIST test vector — let us
/// avoid hashing things in test code and instead anchor against a known value.
const SHA256_OF_ABC: &str = "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad";

/// SHA-256 of "hello\n".  Convenient short fixture independent of any
/// platform-specific line ending behaviour because we write exact bytes.
const SHA256_OF_HELLO_NL: &str =
    "5891b5b522d5df086d0ff0b110fbd9d21bb4fc7163af34d08286a2e846f6be03";

// ── list_files_with_inodes ──────────────────────────────────────────────────

#[test]
fn list_files_with_inodes_returns_relative_paths_only() {
    if !have_binary("find") {
        eprintln!("skipping: find not available");
        return;
    }

    let dest = TempDir::new().unwrap();
    let snap_name = "2026-05-22-100000";
    let snap = make_snapshot_dir(dest.path(), snap_name);

    fs::create_dir_all(snap.join("etc")).unwrap();
    fs::create_dir_all(snap.join("home/me")).unwrap();
    fs::write(snap.join("etc/hosts"), b"127.0.0.1 localhost\n").unwrap();
    fs::write(snap.join("home/me/foo.txt"), b"hi\n").unwrap();
    fs::write(snap.join("toplevel.txt"), b"top\n").unwrap();

    let backend = backend_for(dest.path());
    let listing = backend.list_files_with_inodes(snap_name).unwrap();

    let paths: HashSet<String> = listing.iter().map(|(_, p)| p.clone()).collect();
    assert_eq!(
        paths,
        HashSet::from([
            "etc/hosts".to_string(),
            "home/me/foo.txt".to_string(),
            "toplevel.txt".to_string(),
        ]),
        "should list every regular file with paths relative to the snapshot root"
    );
}

#[test]
fn list_files_with_inodes_returns_correct_inode_numbers() {
    if !have_binary("find") {
        eprintln!("skipping: find not available");
        return;
    }

    let dest = TempDir::new().unwrap();
    let snap_name = "2026-05-22-100000";
    let snap = make_snapshot_dir(dest.path(), snap_name);

    let file_path = snap.join("only-file.txt");
    fs::write(&file_path, b"hello\n").unwrap();
    let expected_inode = fs::metadata(&file_path).unwrap().ino();

    let backend = backend_for(dest.path());
    let listing = backend.list_files_with_inodes(snap_name).unwrap();

    assert_eq!(listing.len(), 1);
    assert_eq!(listing[0].0, expected_inode, "inode must match `stat`");
    assert_eq!(listing[0].1, "only-file.txt");
}

#[test]
fn list_files_with_inodes_sees_hard_links_with_same_inode() {
    if !have_binary("find") {
        eprintln!("skipping: find not available");
        return;
    }

    let dest = TempDir::new().unwrap();
    let snap_name = "2026-05-22-100000";
    let snap = make_snapshot_dir(dest.path(), snap_name);

    let original = snap.join("a.txt");
    let linked = snap.join("b.txt");
    fs::write(&original, b"same content\n").unwrap();
    fs::hard_link(&original, &linked).unwrap();

    let backend = backend_for(dest.path());
    let listing = backend.list_files_with_inodes(snap_name).unwrap();
    assert_eq!(listing.len(), 2);

    // Both entries share an inode — the core invariant the cache relies on.
    let inode_a = listing.iter().find(|(_, p)| p == "a.txt").unwrap().0;
    let inode_b = listing.iter().find(|(_, p)| p == "b.txt").unwrap().0;
    assert_eq!(
        inode_a, inode_b,
        "hard-linked files must report the same inode"
    );
}

#[test]
fn list_files_with_inodes_handles_spaces_in_paths() {
    if !have_binary("find") {
        eprintln!("skipping: find not available");
        return;
    }

    let dest = TempDir::new().unwrap();
    let snap_name = "2026-05-22-100000";
    let snap = make_snapshot_dir(dest.path(), snap_name);

    fs::create_dir_all(snap.join("dir with spaces")).unwrap();
    fs::write(snap.join("dir with spaces/file with spaces.txt"), b"x").unwrap();

    let backend = backend_for(dest.path());
    let listing = backend.list_files_with_inodes(snap_name).unwrap();
    assert_eq!(listing.len(), 1);
    assert_eq!(listing[0].1, "dir with spaces/file with spaces.txt");
}

#[test]
fn list_files_with_inodes_empty_snapshot_returns_empty() {
    if !have_binary("find") {
        eprintln!("skipping: find not available");
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
fn hash_files_returns_known_sha256_values() {
    if !have_binary("sha256sum") || !have_binary("xargs") {
        eprintln!("skipping: sha256sum / xargs not available");
        return;
    }

    let dest = TempDir::new().unwrap();
    let snap_name = "2026-05-22-100000";
    let snap = make_snapshot_dir(dest.path(), snap_name);

    fs::write(snap.join("abc.txt"), b"abc").unwrap();
    fs::write(snap.join("hello.txt"), b"hello\n").unwrap();

    let backend = backend_for(dest.path());
    let needs = vec![
        NeedsHashing {
            rel_path: "abc.txt".to_string(),
            inode: 1,
        },
        NeedsHashing {
            rel_path: "hello.txt".to_string(),
            inode: 2,
        },
    ];
    let hashed: Vec<HashedEntry> = backend.hash_files(snap_name, &needs).unwrap();
    assert_eq!(hashed.len(), 2);

    // Order must match the input list so the orchestration layer can rely on it.
    assert_eq!(hashed[0].rel_path, "abc.txt");
    assert_eq!(hashed[0].inode, 1);
    assert_eq!(hashed[0].hash, SHA256_OF_ABC);

    assert_eq!(hashed[1].rel_path, "hello.txt");
    assert_eq!(hashed[1].inode, 2);
    assert_eq!(hashed[1].hash, SHA256_OF_HELLO_NL);
}

#[test]
fn hash_files_empty_input_skips_subprocess() {
    let dest = TempDir::new().unwrap();
    let snap_name = "2026-05-22-100000";
    make_snapshot_dir(dest.path(), snap_name);

    let backend = backend_for(dest.path());
    // No find/sha256sum required for the empty case — we want to confirm
    // the early-return path is exercised even on hosts without coreutils.
    let result = backend.hash_files(snap_name, &[]).unwrap();
    assert!(result.is_empty());
}

#[test]
fn hash_files_handles_paths_with_spaces() {
    if !have_binary("sha256sum") || !have_binary("xargs") {
        eprintln!("skipping: sha256sum / xargs not available");
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

#[test]
fn hash_files_hard_linked_files_get_identical_hashes() {
    if !have_binary("sha256sum") || !have_binary("xargs") {
        eprintln!("skipping: sha256sum / xargs not available");
        return;
    }

    let dest = TempDir::new().unwrap();
    let snap_name = "2026-05-22-100000";
    let snap = make_snapshot_dir(dest.path(), snap_name);

    fs::write(snap.join("a.txt"), b"abc").unwrap();
    fs::hard_link(snap.join("a.txt"), snap.join("b.txt")).unwrap();

    let backend = backend_for(dest.path());
    let needs = vec![
        NeedsHashing {
            rel_path: "a.txt".to_string(),
            inode: 1,
        },
        NeedsHashing {
            rel_path: "b.txt".to_string(),
            inode: 1,
        },
    ];
    let hashed = backend.hash_files(snap_name, &needs).unwrap();
    assert_eq!(hashed[0].hash, SHA256_OF_ABC);
    assert_eq!(hashed[1].hash, SHA256_OF_ABC);
}

// ── checksums.db (read/write) ───────────────────────────────────────────────

#[test]
fn read_checksum_db_returns_none_when_missing() {
    let dest = TempDir::new().unwrap();
    let backend = backend_for(dest.path());
    assert!(backend.read_checksum_db().unwrap().is_none());
}

#[test]
fn write_then_read_checksum_db_roundtrips() {
    let dest = TempDir::new().unwrap();
    let backend = backend_for(dest.path());

    let db = ChecksumDb::from_entries(&[HashedEntry {
        rel_path: "etc/hosts".to_string(),
        inode: 1234,
        hash: "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855".to_string(),
    }]);
    let serialized = db.serialize();
    backend.write_checksum_db(&serialized).unwrap();

    let reloaded = backend.read_checksum_db().unwrap().expect("file exists");
    assert_eq!(reloaded, serialized);

    // Parsing it back must succeed and contain the same row.
    let parsed = ChecksumDb::load(&reloaded).unwrap();
    assert_eq!(parsed.get("etc/hosts").unwrap().inode, 1234);
}

#[test]
fn write_checksum_db_leaves_no_tmp_file_behind() {
    let dest = TempDir::new().unwrap();
    let backend = backend_for(dest.path());

    backend.write_checksum_db("# justfolders-checksum-db v1\n").unwrap();

    assert!(dest.path().join("checksums.db").exists());
    assert!(
        !dest.path().join("checksums.db.tmp").exists(),
        "atomic write must leave no .tmp sibling on success"
    );
}

#[test]
fn write_checksum_db_overwrites_existing_atomically() {
    let dest = TempDir::new().unwrap();
    let backend = backend_for(dest.path());

    backend.write_checksum_db("first\n").unwrap();
    backend.write_checksum_db("second\n").unwrap();

    let content = backend.read_checksum_db().unwrap().unwrap();
    assert_eq!(content, "second\n");
}

// ── per-snapshot sha256sum (read/write) ─────────────────────────────────────

#[test]
fn read_snapshot_sha256_returns_none_when_missing() {
    let dest = TempDir::new().unwrap();
    let snap_name = "2026-05-22-100000";
    make_snapshot_dir(dest.path(), snap_name);

    let backend = backend_for(dest.path());
    assert!(backend.read_snapshot_sha256(snap_name).unwrap().is_none());
}

#[test]
fn write_then_read_snapshot_sha256_roundtrips() {
    let dest = TempDir::new().unwrap();
    let snap_name = "2026-05-22-100000";
    make_snapshot_dir(dest.path(), snap_name);
    let backend = backend_for(dest.path());

    let content = format!("{SHA256_OF_ABC}  etc/hosts\n");
    backend.write_snapshot_sha256(snap_name, &content).unwrap();

    let read_back = backend.read_snapshot_sha256(snap_name).unwrap().unwrap();
    assert_eq!(read_back, content);
}

#[test]
fn write_snapshot_sha256_leaves_no_tmp_file_behind() {
    let dest = TempDir::new().unwrap();
    let snap_name = "2026-05-22-100000";
    let snap = make_snapshot_dir(dest.path(), snap_name);
    let backend = backend_for(dest.path());

    backend.write_snapshot_sha256(snap_name, "x\n").unwrap();
    assert!(snap.join("sha256sum").exists());
    assert!(!snap.join("sha256sum.tmp").exists());
}

#[test]
fn write_snapshot_sha256_rejects_bad_snapshot_name() {
    let dest = TempDir::new().unwrap();
    let backend = backend_for(dest.path());

    // Path traversal would be very bad — confirm the existing name
    // validator catches it before any I/O touches disk.
    let err = backend
        .write_snapshot_sha256("../etc", "x")
        .expect_err("must reject path traversal");
    let msg = err.to_string();
    assert!(
        msg.contains("invalid snapshot name"),
        "expected snapshot-name rejection, got: {msg}"
    );
}

#[test]
fn write_snapshot_sha256_output_is_sha256sum_check_compatible() {
    if !have_binary("sha256sum") {
        eprintln!("skipping: sha256sum not available");
        return;
    }

    // End-to-end smoke test: write a snapshot file using our backend, then
    // verify it with the stock `sha256sum -c` binary the user would run by
    // hand.  This is the contract step 5's `verify` command will rely on.
    let dest = TempDir::new().unwrap();
    let snap_name = "2026-05-22-100000";
    let snap = make_snapshot_dir(dest.path(), snap_name);
    fs::write(snap.join("payload.txt"), b"abc").unwrap();

    let backend = backend_for(dest.path());
    let content =
        justfolders::core::checksum::format_sha256sum_file(&[HashedEntry {
            rel_path: "payload.txt".to_string(),
            inode: 0,
            hash: SHA256_OF_ABC.to_string(),
        }]);
    backend.write_snapshot_sha256(snap_name, &content).unwrap();

    let status = std::process::Command::new("sha256sum")
        .arg("-c")
        .arg("sha256sum")
        .current_dir(&snap)
        .status()
        .expect("run sha256sum -c");
    assert!(status.success(), "sha256sum -c must accept our file");
}
