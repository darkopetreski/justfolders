//! Checksum cache database and per-snapshot `sha256sum` formatting.
//!
//! This module contains **pure logic only** — no I/O — so it can be
//! exhaustively unit-tested.  Disk reads/writes, `find` invocations and
//! `sha256sum` invocations live in [`crate::core::storage_backend`].
//!
//! Tests live in `tests/checksum.rs` (project convention: integration tests
//! under `tests/`, not inline `#[cfg(test)] mod tests` blocks).
//!
//! # Two artifacts
//!
//! After every successful backup, JustFolders produces two files:
//!
//! 1. **`<dest>/.checksums.db`** — a global cache mapping
//!    *path-relative-to-its-snapshot* → `(inode, sha256)`.  Used to avoid
//!    re-hashing files that are hard-linked from a previous snapshot.  This
//!    file is purely an optimization cache and is regenerated from scratch
//!    on each run.  Its format is internal and not user-facing.
//!
//! 2. **`<snapshot>/sha256sum`** — the standard GNU coreutils `sha256sum`
//!    format.  Lets a user verify a snapshot with `sha256sum -c sha256sum`,
//!    no `justfolders` binary required.  Paths are relative to the snapshot
//!    directory so the command works after a simple `cd`.
//!
//! # Cache lookup model
//!
//! For each file `(inode, rel_path)` found in the new snapshot:
//!
//! * If the cache has a row keyed by `rel_path` **and** the cached inode
//!   matches the file's current inode → the file is hard-linked to the
//!   previously hashed version, so the cached hash can be reused.
//! * Otherwise → recompute the hash from disk.
//!
//! Hard-linked files share data blocks on disk, so "same inode" implies
//! "same content" as long as the inode is still alive.  Inode reuse after
//! retention deletion is theoretically possible (a freed inode could be
//! reassigned to a brand-new file at the same relative path); the practical
//! likelihood of all three coincidences (same path, same freed inode,
//! corrupted-from-the-user's-perspective new content) is vanishingly small
//! and considered acceptable.

use std::collections::HashMap;
use thiserror::Error;

// ── Constants ────────────────────────────────────────────────────────────────

/// Header marker written at the top of every cache file.  Bumping the version
/// suffix forces a rebuild — older files fail [`ChecksumDb::load`] and the
/// caller should treat that as "start from an empty cache".
pub const DB_HEADER: &str = "# justfolders-checksum-db v1";

/// Field separator inside the cache file.  Tab is rare in real paths and the
/// file is internal so we control what gets written.  Paths containing tabs
/// or newlines are skipped on serialization (with a warning at the
/// orchestration layer) — those files will simply be re-hashed every backup.
const DB_SEP: char = '\t';

/// Length of a SHA-256 digest expressed as lowercase hex.
const SHA256_HEX_LEN: usize = 64;

// ── Errors ───────────────────────────────────────────────────────────────────

/// Errors returned when parsing a cache file.
#[derive(Debug, Error, PartialEq, Eq)]
pub enum ChecksumDbError {
    #[error("malformed .checksums.db: {0}")]
    Malformed(String),
}

// ── Types ────────────────────────────────────────────────────────────────────

/// A row stored in the cache, keyed by the file's path relative to its
/// snapshot directory.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CacheRow {
    pub inode: u64,
    pub hash: String,
}

/// A file in a snapshot for which we know — or have just computed — the
/// SHA-256 hash.  Used both as the output of the planning step (entries
/// reused from the cache) and as the input fed back in once the hashing
/// backend has computed the rest.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HashedEntry {
    pub rel_path: String,
    pub inode: u64,
    pub hash: String,
}

/// A file that the planning step decided cannot be reused from the cache and
/// must be hashed afresh.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NeedsHashing {
    pub rel_path: String,
    pub inode: u64,
}

/// Partitioning of a snapshot listing against the cache.
///
/// `reuse` carries entries whose hash is already known.  `needs_hashing`
/// carries entries that the backend must hash from disk.  Concatenating both
/// (after hashing) gives the complete set of files in the snapshot.
#[derive(Debug, Default)]
pub struct Plan {
    pub reuse: Vec<HashedEntry>,
    pub needs_hashing: Vec<NeedsHashing>,
}

// ── ChecksumDb ───────────────────────────────────────────────────────────────

/// In-memory image of the global checksum cache (`<dest>/.checksums.db`).
///
/// Keyed by path relative to the snapshot directory (e.g. `etc/hosts`, not
/// `/dest/2026-05-22-100000/etc/hosts`).
#[derive(Debug, Default)]
pub struct ChecksumDb {
    rows: HashMap<String, CacheRow>,
}

impl ChecksumDb {
    /// Returns an empty cache.  Used when no `.checksums.db` exists yet (first
    /// backup) or when the existing file is unreadable (recovery path).
    pub fn empty() -> Self {
        Self::default()
    }

    /// Number of entries in the cache.
    pub fn len(&self) -> usize {
        self.rows.len()
    }

    /// `true` if the cache contains no entries.
    pub fn is_empty(&self) -> bool {
        self.rows.is_empty()
    }

    /// Returns the cached row for `rel_path`, if any.
    pub fn get(&self, rel_path: &str) -> Option<&CacheRow> {
        self.rows.get(rel_path)
    }

    /// Builds a cache from a complete set of hashed entries.
    ///
    /// The orchestration layer calls this after each backup with the merged
    /// set of (reused + freshly hashed) entries — the new cache then mirrors
    /// the *current* snapshot exactly, so paths that have disappeared (e.g.
    /// because their snapshot was pruned by retention) do not accumulate.
    ///
    /// If `entries` contains duplicate `rel_path` values, the last one wins.
    pub fn from_entries(entries: &[HashedEntry]) -> Self {
        let mut rows = HashMap::with_capacity(entries.len());
        for e in entries {
            rows.insert(
                e.rel_path.clone(),
                CacheRow {
                    inode: e.inode,
                    hash: e.hash.clone(),
                },
            );
        }
        Self { rows }
    }

    /// Parses the textual cache file content.
    ///
    /// Returns [`ChecksumDbError::Malformed`] for missing/unsupported header,
    /// malformed rows, non-hex hashes, or hashes of wrong length.  The caller
    /// can treat any error as "start from an empty cache and rebuild" — at
    /// worst this re-hashes every file on the next backup.
    pub fn load(content: &str) -> Result<Self, ChecksumDbError> {
        let mut lines = content.lines();

        // First non-empty line must be the version header.  We intentionally
        // do not skip blank/comment lines before it: the file is machine
        // written and the header is always present at the top.
        let header = lines.next().ok_or_else(|| {
            ChecksumDbError::Malformed("file is empty (no header line)".to_string())
        })?;

        if header != DB_HEADER {
            return Err(ChecksumDbError::Malformed(format!(
                "unsupported header line: '{header}' (expected '{DB_HEADER}')"
            )));
        }

        let mut rows: HashMap<String, CacheRow> = HashMap::new();
        // Line numbers start at 2 because the header was line 1.
        for (idx, line) in lines.enumerate() {
            let line_num = idx + 2;
            if line.is_empty() {
                continue;
            }

            // splitn(3, '\t') so the path itself may contain '\t' verbatim
            // — though [`serialize`] never emits such paths.  Future-proofing.
            let mut parts = line.splitn(3, DB_SEP);
            let inode_str = parts.next().ok_or_else(|| {
                ChecksumDbError::Malformed(format!("line {line_num}: missing inode field"))
            })?;
            let hash = parts.next().ok_or_else(|| {
                ChecksumDbError::Malformed(format!("line {line_num}: missing hash field"))
            })?;
            let path = parts.next().ok_or_else(|| {
                ChecksumDbError::Malformed(format!("line {line_num}: missing path field"))
            })?;

            let inode: u64 = inode_str.parse().map_err(|_| {
                ChecksumDbError::Malformed(format!(
                    "line {line_num}: invalid inode '{inode_str}'"
                ))
            })?;

            if hash.len() != SHA256_HEX_LEN || !hash.chars().all(|c| c.is_ascii_hexdigit()) {
                return Err(ChecksumDbError::Malformed(format!(
                    "line {line_num}: invalid sha256 hash '{hash}'"
                )));
            }

            rows.insert(
                path.to_string(),
                CacheRow {
                    inode,
                    hash: hash.to_string(),
                },
            );
        }

        Ok(Self { rows })
    }

    /// Renders the cache as a single string ready to be written to
    /// `<dest>/.checksums.db`.
    ///
    /// Output is sorted by relative path so the file is byte-stable across
    /// runs (helps `diff`, snapshot tests, and version-control inspection).
    ///
    /// Paths containing the field separator (`\t`) or a newline are skipped
    /// — the textual format cannot represent them safely.  Such files will
    /// simply be re-hashed on every backup; they are still recorded in the
    /// per-snapshot `sha256sum` file, which uses coreutils escaping.
    pub fn serialize(&self) -> String {
        let mut entries: Vec<(&String, &CacheRow)> = self.rows.iter().collect();
        entries.sort_by(|a, b| a.0.cmp(b.0));

        let mut out = String::with_capacity(DB_HEADER.len() + 1 + entries.len() * 96);
        out.push_str(DB_HEADER);
        out.push('\n');

        for (path, row) in entries {
            if path.contains(DB_SEP) || path.contains('\n') {
                continue;
            }
            out.push_str(&row.inode.to_string());
            out.push(DB_SEP);
            out.push_str(&row.hash);
            out.push(DB_SEP);
            out.push_str(path);
            out.push('\n');
        }

        out
    }

    /// Partitions a snapshot listing into "reuse" and "needs hashing".
    ///
    /// `listing` is the raw output of `find -type f -printf '%i %p\0'`,
    /// already split into `(inode, rel_path)` tuples where `rel_path` is
    /// relative to the snapshot directory.
    pub fn plan(&self, listing: &[(u64, String)]) -> Plan {
        let mut reuse = Vec::new();
        let mut needs_hashing = Vec::new();

        for (inode, rel_path) in listing {
            match self.rows.get(rel_path) {
                // Cache hit and inode unchanged — the file is the same on-disk
                // bytes as last time, so we can reuse the cached hash.
                Some(row) if row.inode == *inode => {
                    reuse.push(HashedEntry {
                        rel_path: rel_path.clone(),
                        inode: *inode,
                        hash: row.hash.clone(),
                    });
                }
                // Cache miss or stale inode → hash the file from disk.
                _ => {
                    needs_hashing.push(NeedsHashing {
                        rel_path: rel_path.clone(),
                        inode: *inode,
                    });
                }
            }
        }

        Plan {
            reuse,
            needs_hashing,
        }
    }
}

// ── Per-snapshot sha256sum file formatter ────────────────────────────────────

/// Renders the per-snapshot `sha256sum` file using the GNU coreutils format,
/// the same one produced by `sha256sum <file>` and consumed by
/// `sha256sum -c`.
///
/// # Format
///
/// Normal entry:
/// ```text
/// <64-hex-hash>  <relative-path>\n
/// ```
/// (hash, two spaces, path, LF).
///
/// If a path contains `\` or `\n`, the line is prefixed with `\` to signal
/// "the path that follows is escaped", and inside the path:
/// * `\`  becomes `\\`
/// * LF   becomes `\n` (literal backslash followed by the letter `n`)
///
/// This is exactly what `sha256sum` itself emits for such filenames, so the
/// file remains a drop-in input for `sha256sum -c`.
///
/// Entries are sorted by relative path so the file is byte-stable across
/// runs.
pub fn format_sha256sum_file(entries: &[HashedEntry]) -> String {
    let mut sorted: Vec<&HashedEntry> = entries.iter().collect();
    sorted.sort_by(|a, b| a.rel_path.cmp(&b.rel_path));

    // Rough capacity hint: 64 hex + 2 spaces + path + LF, plus occasional
    // escapes.  Slightly overshooting is fine — String reallocs cheaply.
    let mut out = String::with_capacity(sorted.iter().map(|e| 67 + e.rel_path.len()).sum());

    for entry in sorted {
        let needs_escape = entry.rel_path.contains('\\') || entry.rel_path.contains('\n');
        if needs_escape {
            out.push('\\');
        }
        out.push_str(&entry.hash);
        out.push_str("  ");
        if needs_escape {
            for ch in entry.rel_path.chars() {
                match ch {
                    '\\' => out.push_str("\\\\"),
                    '\n' => out.push_str("\\n"),
                    c => out.push(c),
                }
            }
        } else {
            out.push_str(&entry.rel_path);
        }
        out.push('\n');
    }

    out
}
