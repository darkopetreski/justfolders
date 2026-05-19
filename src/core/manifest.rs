use rayon::prelude::*;
use sha2::{Digest, Sha256};
use std::ffi::OsStr;
use std::fs::{self, File};
use std::io::{self, BufRead, BufReader, BufWriter, Read, Write};
use std::path::{Path, PathBuf};

pub const MANIFEST_FILENAME: &str = "SHA256SUMS";

// ── Public types ─────────────────────────────────────────────────────────────

/// A single verification failure found when checking a snapshot's manifest.
#[derive(Debug)]
pub enum VerifyFailure {
    /// The file exists but its checksum no longer matches the recorded value.
    ChecksumMismatch {
        path: String,
        expected: String,
        actual: String,
    },
    /// The file is listed in the manifest but is missing from disk.
    Missing { path: String },
}

/// Result of a [`verify_local`] or [`StorageBackend::verify_snapshot`] call.
#[derive(Debug)]
pub struct VerifyReport {
    /// Number of files whose checksum matched the manifest.
    pub ok_count: usize,
    pub failures: Vec<VerifyFailure>,
}

impl VerifyReport {
    /// Returns `true` when no checksum mismatches or missing files were found.
    pub fn is_clean(&self) -> bool {
        self.failures.is_empty()
    }
}

// ── Manifest generation ───────────────────────────────────────────────────────

/// Walks `snapshot_dir` recursively and writes a `SHA256SUMS` manifest file inside it.
///
/// Output is byte-identical to the remote command
/// `find -P . -type f ! -name 'SHA256SUMS' -print0 | LC_ALL=C sort -z | xargs -0 sha256sum`:
///
/// - Symlinks are never followed (matches `find -P` + `-type f`).
/// - Any file whose basename equals `SHA256SUMS` is excluded at every depth
///   (matches `find -name SHA256SUMS`, not just the snapshot-root file).
/// - Entries are sorted by raw path bytes (matches `LC_ALL=C sort -z`).
/// - Each line is `<hex_hash>  ./<relative_path>\n`, written from the raw
///   filesystem bytes so non-UTF-8 names round-trip exactly.
///
/// Hashing runs in parallel across the rayon thread pool, which is what makes
/// this materially faster than a serial `sha256sum` on snapshots with many
/// small files (the dominant cost is per-file open/read, not the hash itself).
pub fn generate_local(snapshot_dir: &Path) -> io::Result<()> {
    let mut entries: Vec<(Vec<u8>, PathBuf)> = Vec::new(); // (rel_bytes, abs_path)
    collect_files(snapshot_dir, snapshot_dir, &mut entries)?;

    // Byte-wise sort matches `LC_ALL=C sort -z` on the remote.
    entries.sort_unstable_by(|a, b| a.0.cmp(&b.0));

    // Hash in parallel; rayon preserves input order in the collected Vec.
    let hashed: Vec<(Vec<u8>, String)> = entries
        .into_par_iter()
        .map(|(rel, abs)| compute_hash(&abs).map(|h| (rel, h)))
        .collect::<io::Result<Vec<_>>>()?;

    let manifest_path = snapshot_dir.join(MANIFEST_FILENAME);
    let mut out = BufWriter::new(File::create(&manifest_path)?);
    for (rel, hash) in &hashed {
        // sha256sum format: "<hex>  ./<rel>\n" (two spaces separator).
        out.write_all(hash.as_bytes())?;
        out.write_all(b"  ./")?;
        out.write_all(rel)?;
        out.write_all(b"\n")?;
    }
    out.flush()?;

    Ok(())
}

/// Recursively walks `dir` and appends `(relative_path_bytes, absolute_path)`
/// pairs to `out` for every regular file (symlinks and the `SHA256SUMS`
/// manifest file are skipped).
///
/// `root` is the snapshot root used to compute relative paths; it stays fixed
/// across the recursive calls while `dir` advances into sub-directories.
///
/// Relative paths are stored as raw bytes so the sort order and the manifest
/// output match `find -print0 | LC_ALL=C sort -z` exactly even when filenames
/// contain non-UTF-8 byte sequences.
fn collect_files(
    root: &Path,
    dir: &Path,
    out: &mut Vec<(Vec<u8>, PathBuf)>,
) -> io::Result<()> {
    for entry in fs::read_dir(dir)? {
        let entry = entry?;
        let file_type = entry.file_type()?;

        // Never follow symlinks — they are directory structure, not file content,
        // and following them could cause the walk to escape the snapshot directory.
        if file_type.is_symlink() {
            continue;
        }

        let path = entry.path();

        if file_type.is_dir() {
            collect_files(root, &path, out)?;
        } else if file_type.is_file() {
            // Match `find -name 'SHA256SUMS'` semantics: exclude by basename
            // at any depth, not just at the snapshot root.
            if entry.file_name() == OsStr::new(MANIFEST_FILENAME) {
                continue;
            }

            let rel = path
                .strip_prefix(root)
                .map_err(io::Error::other)?;
            out.push((os_str_to_bytes(rel.as_os_str()), path));
        }
    }
    Ok(())
}

/// Returns the raw bytes of `s` on Unix (matching what `find` would emit) and
/// a UTF-8 lossy fallback elsewhere (where this manifest format is not really
/// meaningful anyway, since the remote command relies on Unix tools).
fn os_str_to_bytes(s: &OsStr) -> Vec<u8> {
    #[cfg(unix)]
    {
        use std::os::unix::ffi::OsStrExt;
        s.as_bytes().to_vec()
    }
    #[cfg(not(unix))]
    {
        s.to_string_lossy().into_owned().into_bytes()
    }
}

// ── Manifest verification ─────────────────────────────────────────────────────

/// Reads the `SHA256SUMS` manifest inside `snapshot_dir` and re-checksums every
/// listed file, returning a [`VerifyReport`] describing any discrepancies.
pub fn verify_local(snapshot_dir: &Path) -> io::Result<VerifyReport> {
    let manifest_path = snapshot_dir.join(MANIFEST_FILENAME);
    let file = File::open(&manifest_path)?;
    let reader = BufReader::new(file);

    let mut ok_count = 0usize;
    let mut failures = Vec::new();

    for line in reader.lines() {
        let line = line?;
        let line = line.trim();
        if line.is_empty() {
            continue;
        }

        let (expected_hash, path_str) = parse_line(line)?;

        // Paths in the manifest are stored as "./<relative>"; strip the prefix.
        let rel = path_str.trim_start_matches("./");

        // Reject any path that could escape the snapshot directory.  A tampered
        // manifest containing ".." components could otherwise trick the verifier
        // into hashing files outside the snapshot (information disclosure) or
        // following an attacker-controlled absolute path.
        if rel.contains("..") || rel.starts_with('/') {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                format!("manifest contains unsafe path: {path_str}"),
            ));
        }

        let abs = snapshot_dir.join(rel);
        // Defense-in-depth: confirm the resolved path is still inside the snapshot.
        if !abs.starts_with(snapshot_dir) {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                format!("manifest path escapes snapshot directory: {path_str}"),
            ));
        }

        if !abs.exists() {
            failures.push(VerifyFailure::Missing {
                path: path_str.to_string(),
            });
            continue;
        }

        let actual_hash = compute_hash(&abs)?;
        if actual_hash == expected_hash {
            ok_count += 1;
        } else {
            failures.push(VerifyFailure::ChecksumMismatch {
                path: path_str.to_string(),
                expected: expected_hash.to_string(),
                actual: actual_hash,
            });
        }
    }

    Ok(VerifyReport { ok_count, failures })
}

// ── Helpers ───────────────────────────────────────────────────────────────────

/// Computes the SHA-256 hex digest of the file at `path`.
fn compute_hash(path: &Path) -> io::Result<String> {
    let mut file = File::open(path)?;
    let mut hasher = Sha256::new();
    let mut buf = [0u8; 65536];
    loop {
        let n = file.read(&mut buf)?;
        if n == 0 {
            break;
        }
        hasher.update(&buf[..n]);
    }
    Ok(hex::encode(hasher.finalize()))
}

/// Parses one manifest line of the form `"<hash>  <path>"`.
fn parse_line(line: &str) -> io::Result<(&str, &str)> {
    // Standard sha256sum format uses two spaces between hash and filename.
    match line.find("  ") {
        Some(pos) => Ok((&line[..pos], &line[pos + 2..])),
        None => Err(io::Error::new(
            io::ErrorKind::InvalidData,
            format!("malformed manifest line: {line}"),
        )),
    }
}
