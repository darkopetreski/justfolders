//! Integration tests for [`justfolders::core::checksum_step::run`].
//!
//! These exercise the orchestration that runs once at the end of every
//! backup: load → list → plan → hash → write artifacts.  We construct
//! snapshot directories by hand (no rsync involvement) so the tests are
//! fast and reproducible; the actual backup-level test that exercises
//! rsync + the checksum step together lives in `tests/backup.rs`.

use justfolders::core::checksum::ChecksumDb;
use justfolders::core::checksum_step;
use justfolders::core::config::{Config, RetentionPolicy};
use justfolders::core::storage_backend::{from_config, StorageBackend};
use std::fs;
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
        checksum: true,
    }
}

fn backend_for(dest_dir: &Path) -> Box<dyn StorageBackend> {
    let config = make_local_config(&dest_dir.display().to_string());
    from_config(&config)
}

/// `true` if the named binary is on `$PATH`.  Tests that need real
/// `find` / `sha256sum` / `xargs` invocations skip themselves with a
/// printed note on hosts that lack coreutils/findutils.
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

fn coreutils_available() -> bool {
    have_binary("find") && have_binary("xargs") && have_binary("sha256sum")
}

/// Creates `<dest>/<name>/` and returns the absolute path to the new dir.
fn make_snapshot_dir(dest: &Path, name: &str) -> std::path::PathBuf {
    let snap = dest.join(name);
    fs::create_dir(&snap).expect("create snapshot dir");
    snap
}

/// Known SHA-256 of the literal byte string `"abc"`.  Standard NIST test
/// vector — used here as an anchor so the assertions stay readable.
const SHA256_OF_ABC: &str = "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad";

// ── Tests ───────────────────────────────────────────────────────────────────

#[test]
fn first_snapshot_hashes_every_file_and_writes_both_artifacts() {
    if !coreutils_available() {
        eprintln!("skipping: find / xargs / sha256sum not available");
        return;
    }

    let dest = TempDir::new().unwrap();
    let snap_name = "2026-05-22-100000";
    let snap = make_snapshot_dir(dest.path(), snap_name);
    fs::write(snap.join("a.txt"), b"abc").unwrap();
    fs::write(snap.join("b.txt"), b"abc").unwrap(); // identical content, different inode
    fs::create_dir_all(snap.join("sub")).unwrap();
    fs::write(snap.join("sub/c.txt"), b"abc").unwrap();

    let backend = backend_for(dest.path());
    let summary = checksum_step::run(backend.as_ref(), snap_name).expect("run");

    assert_eq!(summary.hashed, 3, "first run: every file must be hashed");
    assert_eq!(summary.reused, 0);
    assert_eq!(summary.total(), 3);

    // Per-snapshot file lists every path.  Standard sha256sum format means
    // it is also valid input to `sha256sum -c`, which we assert below.
    let sha_file = snap.join("sha256sum");
    let content = fs::read_to_string(&sha_file).unwrap();
    for path in ["a.txt", "b.txt", "sub/c.txt"] {
        let line = format!("{SHA256_OF_ABC}  {path}");
        assert!(
            content.lines().any(|l| l == line),
            "missing expected line '{line}' in:\n{content}"
        );
    }

    // Global cache exists at the destination root and contains every path.
    let db_content = fs::read_to_string(dest.path().join(".checksums.db")).unwrap();
    let db = ChecksumDb::load(&db_content).expect("parse .checksums.db");
    assert_eq!(db.len(), 3);
    assert!(db.get("a.txt").is_some());
    assert!(db.get("b.txt").is_some());
    assert!(db.get("sub/c.txt").is_some());

    // No leftover .tmp siblings.
    assert!(!snap.join("sha256sum.tmp").exists());
    assert!(!dest.path().join(".checksums.db.tmp").exists());

    // End-to-end interop with the stock `sha256sum -c` binary.
    let status = std::process::Command::new("sha256sum")
        .arg("-c")
        .arg("sha256sum")
        .current_dir(&snap)
        .status()
        .expect("run sha256sum -c");
    assert!(status.success(), "sha256sum -c must accept our file");
}

#[test]
fn second_snapshot_with_hardlinks_reuses_all_cached_hashes() {
    if !coreutils_available() {
        eprintln!("skipping: find / xargs / sha256sum not available");
        return;
    }

    let dest = TempDir::new().unwrap();
    let backend = backend_for(dest.path());

    // First snapshot: hashes from scratch.
    let snap1_name = "2026-05-22-100000";
    let snap1 = make_snapshot_dir(dest.path(), snap1_name);
    fs::write(snap1.join("foo.txt"), b"abc").unwrap();
    fs::write(snap1.join("bar.txt"), b"abc").unwrap();
    let s1 = checksum_step::run(backend.as_ref(), snap1_name).expect("first run");
    assert_eq!(s1.hashed, 2);
    assert_eq!(s1.reused, 0);

    // Second snapshot: hard-link from snap1 — mimics what rsync's
    // --link-dest does for unchanged files.  Same inodes ⇒ cache must hit.
    let snap2_name = "2026-05-22-110000";
    let snap2 = make_snapshot_dir(dest.path(), snap2_name);
    fs::hard_link(snap1.join("foo.txt"), snap2.join("foo.txt")).unwrap();
    fs::hard_link(snap1.join("bar.txt"), snap2.join("bar.txt")).unwrap();
    let s2 = checksum_step::run(backend.as_ref(), snap2_name).expect("second run");
    assert_eq!(s2.hashed, 0, "all files should hit the cache");
    assert_eq!(s2.reused, 2);

    // snap2 must have its own sha256sum file with the same hashes.
    let snap2_content = fs::read_to_string(snap2.join("sha256sum")).unwrap();
    assert!(snap2_content.lines().count() == 2);
    assert!(snap2_content.contains(SHA256_OF_ABC));
}

#[test]
fn second_snapshot_with_new_file_hashes_only_the_new_one() {
    if !coreutils_available() {
        eprintln!("skipping: find / xargs / sha256sum not available");
        return;
    }

    let dest = TempDir::new().unwrap();
    let backend = backend_for(dest.path());

    let snap1_name = "2026-05-22-100000";
    let snap1 = make_snapshot_dir(dest.path(), snap1_name);
    fs::write(snap1.join("foo.txt"), b"abc").unwrap();
    let s1 = checksum_step::run(backend.as_ref(), snap1_name).expect("first run");
    assert_eq!(s1.hashed, 1);

    let snap2_name = "2026-05-22-110000";
    let snap2 = make_snapshot_dir(dest.path(), snap2_name);
    // foo.txt is hard-linked → cache hit.
    fs::hard_link(snap1.join("foo.txt"), snap2.join("foo.txt")).unwrap();
    // bar.txt is brand new → must be hashed.
    fs::write(snap2.join("bar.txt"), b"abc").unwrap();

    let s2 = checksum_step::run(backend.as_ref(), snap2_name).expect("second run");
    assert_eq!(s2.reused, 1, "foo.txt should reuse the cached hash");
    assert_eq!(s2.hashed, 1, "bar.txt should be hashed fresh");
    assert_eq!(s2.total(), 2);
}

#[test]
fn cache_mirrors_only_the_current_snapshots_files() {
    if !coreutils_available() {
        eprintln!("skipping: find / xargs / sha256sum not available");
        return;
    }

    let dest = TempDir::new().unwrap();
    let backend = backend_for(dest.path());

    // snap1 has files {a, b}; snap2 has only {a}.  The cache after snap2
    // must contain just {a} — pruned paths do not accumulate.
    let snap1_name = "2026-05-22-100000";
    let snap1 = make_snapshot_dir(dest.path(), snap1_name);
    fs::write(snap1.join("a.txt"), b"abc").unwrap();
    fs::write(snap1.join("b.txt"), b"abc").unwrap();
    checksum_step::run(backend.as_ref(), snap1_name).unwrap();

    let snap2_name = "2026-05-22-110000";
    let snap2 = make_snapshot_dir(dest.path(), snap2_name);
    fs::hard_link(snap1.join("a.txt"), snap2.join("a.txt")).unwrap();
    checksum_step::run(backend.as_ref(), snap2_name).unwrap();

    let db_content = fs::read_to_string(dest.path().join(".checksums.db")).unwrap();
    let db = ChecksumDb::load(&db_content).expect("parse .checksums.db");
    assert_eq!(db.len(), 1, "cache must only carry the current snapshot");
    assert!(db.get("a.txt").is_some());
    assert!(db.get("b.txt").is_none());
}

#[test]
fn malformed_cache_is_treated_as_empty_and_rebuilt() {
    if !coreutils_available() {
        eprintln!("skipping: find / xargs / sha256sum not available");
        return;
    }

    let dest = TempDir::new().unwrap();
    fs::write(
        dest.path().join(".checksums.db"),
        "this is not a valid checksum db\n",
    )
    .unwrap();

    let snap_name = "2026-05-22-100000";
    let snap = make_snapshot_dir(dest.path(), snap_name);
    fs::write(snap.join("only.txt"), b"abc").unwrap();

    let backend = backend_for(dest.path());
    // Must not error — the run self-heals from corruption.
    let summary = checksum_step::run(backend.as_ref(), snap_name).expect("run");
    assert_eq!(summary.hashed, 1);
    assert_eq!(summary.reused, 0);

    // The new cache must be well-formed even though the old one wasn't.
    let db_content = fs::read_to_string(dest.path().join(".checksums.db")).unwrap();
    let db = ChecksumDb::load(&db_content).expect("rebuilt cache must parse");
    assert_eq!(db.len(), 1);
}

#[test]
fn empty_snapshot_produces_empty_artifacts() {
    if !coreutils_available() {
        eprintln!("skipping: find / xargs / sha256sum not available");
        return;
    }

    let dest = TempDir::new().unwrap();
    let snap_name = "2026-05-22-100000";
    let snap = make_snapshot_dir(dest.path(), snap_name);

    let backend = backend_for(dest.path());
    let summary = checksum_step::run(backend.as_ref(), snap_name).expect("run");
    assert_eq!(summary.total(), 0);

    let sha_file = snap.join("sha256sum");
    assert!(sha_file.exists());
    assert_eq!(fs::read_to_string(&sha_file).unwrap(), "");

    let db_content = fs::read_to_string(dest.path().join(".checksums.db")).unwrap();
    // Header-only is a valid, parseable, empty cache.
    let db = ChecksumDb::load(&db_content).unwrap();
    assert!(db.is_empty());
}

#[test]
fn corrupted_snapshot_file_is_detected_by_sha256sum_check() {
    if !coreutils_available() {
        eprintln!("skipping: find / xargs / sha256sum not available");
        return;
    }

    // End-to-end "drift detection" smoke test: after a successful run,
    // tamper with a file and confirm the stock `sha256sum -c` binary
    // surfaces the mismatch.  This is the contract step 5's `verify`
    // command will rely on.
    let dest = TempDir::new().unwrap();
    let snap_name = "2026-05-22-100000";
    let snap = make_snapshot_dir(dest.path(), snap_name);
    fs::write(snap.join("payload.txt"), b"abc").unwrap();

    let backend = backend_for(dest.path());
    checksum_step::run(backend.as_ref(), snap_name).unwrap();

    // Tamper with the file post-checksum.
    fs::write(snap.join("payload.txt"), b"tampered").unwrap();

    let output = std::process::Command::new("sha256sum")
        .arg("-c")
        .arg("sha256sum")
        .current_dir(&snap)
        .output()
        .expect("run sha256sum -c");
    assert!(
        !output.status.success(),
        "sha256sum -c must detect the tampering"
    );
}
