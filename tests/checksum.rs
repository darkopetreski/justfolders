//! Unit tests for the `core::checksum` module.
//!
//! Lives under `tests/` per the project convention (integration tests, not
//! inline `#[cfg(test)] mod tests` blocks).  These cover the pure-logic
//! parts of the checksum subsystem — cache parsing/serialization, the
//! reuse-vs-rehash planning step, and the GNU coreutils `sha256sum` file
//! formatter (including backslash/newline escaping).
//!
//! Disk-level integration (find, sha256sum invocation, atomic writes) is
//! exercised separately by the backup integration tests once steps 2-3 of
//! the implementation land.

use justfolders::core::checksum::{
    format_sha256sum_file, CacheRow, ChecksumDb, ChecksumDbError, HashedEntry,
};

// ── Helpers ─────────────────────────────────────────────────────────────────

// Two syntactically-valid SHA-256 strings.  The actual hash bytes are
// irrelevant because the module under test never hashes — it only stores,
// partitions and renders.  Using fixed constants keeps test assertions short.
const H1: &str = "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855";
const H2: &str = "9c56cc51b374c3ba189210d5b6d4bf57790d351c96c47c02190ecf1e430635ab";

fn entry(rel: &str, inode: u64, hash: &str) -> HashedEntry {
    HashedEntry {
        rel_path: rel.to_string(),
        inode,
        hash: hash.to_string(),
    }
}

// ── ChecksumDb: construction & access ───────────────────────────────────────

#[test]
fn empty_db_is_empty() {
    let db = ChecksumDb::empty();
    assert_eq!(db.len(), 0);
    assert!(db.is_empty());
    assert!(db.get("etc/hosts").is_none());
}

#[test]
fn from_entries_populates_rows() {
    let db = ChecksumDb::from_entries(&[
        entry("etc/hosts", 1234, H1),
        entry("home/me/foo.txt", 5678, H2),
    ]);
    assert_eq!(db.len(), 2);
    assert_eq!(
        db.get("etc/hosts"),
        Some(&CacheRow {
            inode: 1234,
            hash: H1.to_string()
        })
    );
    assert_eq!(
        db.get("home/me/foo.txt"),
        Some(&CacheRow {
            inode: 5678,
            hash: H2.to_string()
        })
    );
}

#[test]
fn from_entries_last_duplicate_wins() {
    let db = ChecksumDb::from_entries(&[
        entry("etc/hosts", 1, H1),
        entry("etc/hosts", 2, H2),
    ]);
    assert_eq!(
        db.get("etc/hosts"),
        Some(&CacheRow {
            inode: 2,
            hash: H2.to_string()
        })
    );
}

// ── ChecksumDb: load ────────────────────────────────────────────────────────

#[test]
fn load_empty_string_fails() {
    let err = ChecksumDb::load("").unwrap_err();
    assert!(matches!(err, ChecksumDbError::Malformed(_)));
}

#[test]
fn load_missing_header_fails() {
    let err = ChecksumDb::load("1234\tdeadbeef\tetc/hosts").unwrap_err();
    assert!(matches!(err, ChecksumDbError::Malformed(_)));
}

#[test]
fn load_wrong_version_fails() {
    let content = "# justfolders-checksum-db v2\n";
    let err = ChecksumDb::load(content).unwrap_err();
    assert!(matches!(err, ChecksumDbError::Malformed(_)));
}

#[test]
fn load_header_only_yields_empty_db() {
    let db = ChecksumDb::load("# justfolders-checksum-db v1\n").unwrap();
    assert!(db.is_empty());
}

#[test]
fn load_parses_rows() {
    let content = format!(
        "# justfolders-checksum-db v1\n1234\t{H1}\tetc/hosts\n5678\t{H2}\thome/me/foo.txt\n"
    );
    let db = ChecksumDb::load(&content).unwrap();
    assert_eq!(db.len(), 2);
    assert_eq!(db.get("etc/hosts").unwrap().inode, 1234);
    assert_eq!(db.get("etc/hosts").unwrap().hash, H1);
    assert_eq!(db.get("home/me/foo.txt").unwrap().inode, 5678);
}

#[test]
fn load_skips_blank_lines() {
    let content = format!(
        "# justfolders-checksum-db v1\n\n1234\t{H1}\tetc/hosts\n\n\n5678\t{H2}\tfoo\n"
    );
    let db = ChecksumDb::load(&content).unwrap();
    assert_eq!(db.len(), 2);
}

#[test]
fn load_rejects_non_numeric_inode() {
    let content = format!("# justfolders-checksum-db v1\nabc\t{H1}\tetc/hosts\n");
    let err = ChecksumDb::load(&content).unwrap_err();
    match err {
        ChecksumDbError::Malformed(msg) => assert!(msg.contains("invalid inode")),
    }
}

#[test]
fn load_rejects_short_hash() {
    let content = "# justfolders-checksum-db v1\n1234\tdeadbeef\tetc/hosts\n";
    let err = ChecksumDb::load(content).unwrap_err();
    match err {
        ChecksumDbError::Malformed(msg) => assert!(msg.contains("invalid sha256 hash")),
    }
}

#[test]
fn load_rejects_non_hex_hash() {
    let bad_hash: String = "z".repeat(64);
    let content = format!("# justfolders-checksum-db v1\n1234\t{bad_hash}\tetc/hosts\n");
    let err = ChecksumDb::load(&content).unwrap_err();
    match err {
        ChecksumDbError::Malformed(msg) => assert!(msg.contains("invalid sha256 hash")),
    }
}

#[test]
fn load_rejects_missing_path() {
    let content = format!("# justfolders-checksum-db v1\n1234\t{H1}\n");
    let err = ChecksumDb::load(&content).unwrap_err();
    match err {
        ChecksumDbError::Malformed(msg) => assert!(msg.contains("missing path")),
    }
}

// ── ChecksumDb: serialize & roundtrip ───────────────────────────────────────

#[test]
fn serialize_empty_db_emits_header_only() {
    let db = ChecksumDb::empty();
    assert_eq!(db.serialize(), "# justfolders-checksum-db v1\n");
}

#[test]
fn serialize_output_is_sorted_by_path() {
    let db = ChecksumDb::from_entries(&[
        entry("z/last", 1, H1),
        entry("a/first", 2, H2),
        entry("m/middle", 3, H1),
    ]);
    let out = db.serialize();
    let path_lines: Vec<&str> = out.lines().skip(1).collect(); // skip header
    assert_eq!(path_lines.len(), 3);
    assert!(path_lines[0].ends_with("\ta/first"));
    assert!(path_lines[1].ends_with("\tm/middle"));
    assert!(path_lines[2].ends_with("\tz/last"));
}

#[test]
fn serialize_skips_paths_with_tab_or_newline() {
    let db = ChecksumDb::from_entries(&[
        entry("ok/path", 1, H1),
        entry("bad\tpath", 2, H2),
        entry("bad\npath", 3, H1),
    ]);
    let out = db.serialize();
    let body_lines: Vec<&str> = out.lines().skip(1).collect();
    assert_eq!(body_lines.len(), 1);
    assert!(body_lines[0].ends_with("\tok/path"));
}

#[test]
fn roundtrip_serialize_then_load() {
    let original = ChecksumDb::from_entries(&[
        entry("etc/hosts", 1234, H1),
        entry("home/me/foo.txt", 5678, H2),
        entry("path with spaces/file", 9999, H1),
    ]);
    let serialized = original.serialize();
    let reloaded = ChecksumDb::load(&serialized).unwrap();
    assert_eq!(reloaded.len(), original.len());
    for path in ["etc/hosts", "home/me/foo.txt", "path with spaces/file"] {
        assert_eq!(reloaded.get(path), original.get(path));
    }
}

// ── ChecksumDb: plan ────────────────────────────────────────────────────────

#[test]
fn plan_empty_cache_hashes_everything() {
    let db = ChecksumDb::empty();
    let listing = vec![(1, "a".to_string()), (2, "b".to_string())];
    let plan = db.plan(&listing);
    assert!(plan.reuse.is_empty());
    assert_eq!(plan.needs_hashing.len(), 2);
}

#[test]
fn plan_cache_hit_with_matching_inode_reuses() {
    let db = ChecksumDb::from_entries(&[entry("etc/hosts", 1234, H1)]);
    let plan = db.plan(&[(1234, "etc/hosts".to_string())]);
    assert_eq!(plan.reuse.len(), 1);
    assert_eq!(plan.reuse[0].rel_path, "etc/hosts");
    assert_eq!(plan.reuse[0].inode, 1234);
    assert_eq!(plan.reuse[0].hash, H1);
    assert!(plan.needs_hashing.is_empty());
}

#[test]
fn plan_cache_hit_with_stale_inode_rehashes() {
    let db = ChecksumDb::from_entries(&[entry("etc/hosts", 1234, H1)]);
    let plan = db.plan(&[(9999, "etc/hosts".to_string())]);
    assert!(plan.reuse.is_empty());
    assert_eq!(plan.needs_hashing.len(), 1);
    assert_eq!(plan.needs_hashing[0].inode, 9999);
}

#[test]
fn plan_cache_miss_rehashes() {
    let db = ChecksumDb::from_entries(&[entry("etc/hosts", 1234, H1)]);
    let plan = db.plan(&[(1, "etc/new-file".to_string())]);
    assert!(plan.reuse.is_empty());
    assert_eq!(plan.needs_hashing.len(), 1);
    assert_eq!(plan.needs_hashing[0].rel_path, "etc/new-file");
}

#[test]
fn plan_mixed_listing() {
    let db = ChecksumDb::from_entries(&[
        entry("a", 1, H1),      // same inode → reuse
        entry("b", 2, H2),      // inode changes → rehash
        entry("gone", 999, H1), // not in listing → ignored entirely
    ]);
    let listing = vec![
        (1, "a".to_string()),  // hit, reuse
        (22, "b".to_string()), // stale, rehash
        (3, "c".to_string()),  // miss, hash
    ];
    let plan = db.plan(&listing);
    assert_eq!(plan.reuse.len(), 1);
    assert_eq!(plan.reuse[0].rel_path, "a");
    assert_eq!(plan.needs_hashing.len(), 2);
    let nh_paths: Vec<&str> = plan
        .needs_hashing
        .iter()
        .map(|e| e.rel_path.as_str())
        .collect();
    assert!(nh_paths.contains(&"b"));
    assert!(nh_paths.contains(&"c"));
}

// ── format_sha256sum_file ───────────────────────────────────────────────────

#[test]
fn format_empty_input_yields_empty_string() {
    assert_eq!(format_sha256sum_file(&[]), "");
}

#[test]
fn format_normal_paths_produce_two_space_delimited_lines() {
    let entries = vec![
        entry("etc/hosts", 1, H1),
        entry("home/me/foo.txt", 2, H2),
    ];
    let out = format_sha256sum_file(&entries);
    let expected = format!("{H1}  etc/hosts\n{H2}  home/me/foo.txt\n");
    assert_eq!(out, expected);
}

#[test]
fn format_sorts_by_path() {
    let entries = vec![
        entry("z", 1, H1),
        entry("a", 2, H2),
        entry("m", 3, H1),
    ];
    let out = format_sha256sum_file(&entries);
    let lines: Vec<&str> = out.lines().collect();
    assert!(lines[0].ends_with("  a"));
    assert!(lines[1].ends_with("  m"));
    assert!(lines[2].ends_with("  z"));
}

#[test]
fn format_escapes_backslash_with_leading_marker() {
    let entries = vec![entry("dir\\with\\bs/file", 1, H1)];
    let out = format_sha256sum_file(&entries);
    // Line is prefixed with '\', and each '\' in the name is doubled.
    let expected = format!("\\{H1}  dir\\\\with\\\\bs/file\n");
    assert_eq!(out, expected);
}

#[test]
fn format_escapes_newline_as_backslash_n() {
    let entries = vec![entry("weird\nname", 1, H1)];
    let out = format_sha256sum_file(&entries);
    // Line is prefixed with '\', and the newline becomes literal "\n".
    let expected = format!("\\{H1}  weird\\nname\n");
    assert_eq!(out, expected);
}

#[test]
fn format_unescaped_path_has_no_leading_marker() {
    let entries = vec![entry("etc/hosts", 1, H1)];
    let out = format_sha256sum_file(&entries);
    assert!(!out.starts_with('\\'));
}

#[test]
fn format_mixed_normal_and_escaped_entries() {
    let entries = vec![
        entry("normal/path", 1, H1),
        entry("weird\\path", 2, H2),
    ];
    let out = format_sha256sum_file(&entries);
    let lines: Vec<&str> = out.lines().collect();
    // Sorted by path: 'normal/path' < 'weird\\path' lexicographically.
    assert_eq!(lines[0], format!("{H1}  normal/path"));
    assert_eq!(lines[1], format!("\\{H2}  weird\\\\path"));
}
