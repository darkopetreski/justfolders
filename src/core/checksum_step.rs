//! Orchestration of the post-rsync checksum step.
//!
//! This is the glue between the pure-logic [`crate::core::checksum`] module
//! and the I/O-bearing [`crate::core::storage_backend`] methods.  It runs
//! once at the end of a successful backup, after `tmp/` has been promoted to
//! a timestamped snapshot directory:
//!
//! 1. Load the previous cache from `<dest>/checksums.db` (treat any
//!    parse error as "start from an empty cache" — the cache is just an
//!    optimization).
//! 2. List every file in the new snapshot together with its inode
//!    (`find -type f -printf '%i %P\0'`).
//! 3. Partition the listing into "reuse from cache" vs "needs hashing"
//!    via [`ChecksumDb::plan`].
//! 4. Hash the "needs hashing" set with `xargs -0 sha256sum`.
//! 5. Write `<snapshot>/sha256sum` in the standard GNU coreutils format so
//!    `sha256sum -c sha256sum` can verify the snapshot without any
//!    JustFolders binary.
//! 6. Rebuild `<dest>/checksums.db` from the merged set of entries so the
//!    cache mirrors the *current* snapshot exactly — pruned paths don't
//!    accumulate.
//!
//! The per-snapshot `sha256sum` file is written **before** the global cache
//! is updated, so a crash between (5) and (6) leaves a fully verifiable
//! snapshot — the cache stays slightly stale but self-heals on the next
//! run.

use crate::core::checksum::{format_sha256sum_file, ChecksumDb, HashedEntry};
use crate::core::storage_backend::{StorageBackend, StorageBackendError};
use thiserror::Error;

// ── Errors ───────────────────────────────────────────────────────────────────

/// Errors raised by the checksum step.
///
/// Currently this is a thin wrapper around [`StorageBackendError`] — every
/// step of the pipeline either calls the backend or runs pure logic that
/// cannot fail.  The orchestration in [`crate::commands::backup`] treats
/// any error here as a warning, not a hard failure: a snapshot without a
/// `sha256sum` file is still a valid snapshot, just unverified.
#[derive(Debug, Error)]
pub enum ChecksumStepError {
    #[error(transparent)]
    Backend(#[from] StorageBackendError),
}

// ── Summary ──────────────────────────────────────────────────────────────────

/// Counts returned by a successful [`run`] for human-readable logging.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ChecksumSummary {
    /// Files whose hash was reused from the previous cache (no disk reads).
    pub reused: usize,
    /// Files that had to be hashed from disk this run.
    pub hashed: usize,
}

impl ChecksumSummary {
    /// Total number of files covered by the resulting `sha256sum` file.
    pub fn total(&self) -> usize {
        self.reused + self.hashed
    }
}

// ── Orchestration ────────────────────────────────────────────────────────────

/// Runs the full checksum step for `snapshot_name` against `backend`.
///
/// See the module-level docs for the step-by-step sequence and the crash
/// safety reasoning behind the artifact write order.
pub fn run(
    backend: &dyn StorageBackend,
    snapshot_name: &str,
) -> Result<ChecksumSummary, ChecksumStepError> {
    let cache = load_cache(backend)?;

    let listing = backend.list_files_with_inodes(snapshot_name)?;
    let plan = cache.plan(&listing);
    let reused_count = plan.reuse.len();
    let to_hash_count = plan.needs_hashing.len();

    let freshly_hashed: Vec<HashedEntry> = if plan.needs_hashing.is_empty() {
        Vec::new()
    } else {
        backend.hash_files(snapshot_name, &plan.needs_hashing)?
    };

    // Merge reused + freshly-hashed into the full set of entries for the
    // current snapshot.  Order does not matter — the per-snapshot file
    // formatter sorts by path, and the cache is keyed by path.
    let mut all: Vec<HashedEntry> = Vec::with_capacity(reused_count + freshly_hashed.len());
    all.extend(plan.reuse);
    all.extend(freshly_hashed);

    // 1) Per-snapshot artifact first — the user-visible one.
    backend.write_snapshot_sha256(snapshot_name, &format_sha256sum_file(&all))?;

    // 2) Then the global cache, rebuilt from scratch so it mirrors the
    //    snapshot we just sealed.  Paths that disappeared between runs
    //    (e.g. files deleted from the source) are not carried over.
    let new_db = ChecksumDb::from_entries(&all);
    backend.write_checksum_db(&new_db.serialize())?;

    Ok(ChecksumSummary {
        reused: reused_count,
        hashed: to_hash_count,
    })
}

/// Reads `<dest>/checksums.db` into memory.
///
/// * Missing file → empty cache (normal on the very first backup).
/// * Malformed content → empty cache + a single-line warning on stderr.
///   This is deliberate: a corrupted cache is just an optimization loss,
///   never a reason to abort a backup or a user-visible failure.
/// * Other I/O errors → propagated.
fn load_cache(backend: &dyn StorageBackend) -> Result<ChecksumDb, ChecksumStepError> {
    match backend.read_checksum_db()? {
        None => Ok(ChecksumDb::empty()),
        Some(content) => match ChecksumDb::load(&content) {
            Ok(db) => Ok(db),
            Err(e) => {
                eprintln!(
                    "warning: checksums.db is malformed ({e}); rebuilding from an empty cache"
                );
                Ok(ChecksumDb::empty())
            }
        },
    }
}
