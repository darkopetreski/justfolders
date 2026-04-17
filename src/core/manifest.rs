use sha2::{Digest, Sha256};
use std::collections::HashMap;
use std::fs::{self, File};
use std::io::{self, BufRead, BufReader, Read, Write};
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
    pub fn is_clean(&self) -> bool {
        self.failures.is_empty()
    }
}

// ── Manifest generation ───────────────────────────────────────────────────────

/// Walks `snapshot_dir` recursively and writes a `SHA256SUMS` manifest file inside it.
///
/// Files are sorted by path for deterministic output.
/// On Unix, files that share an inode (hard-linked from a previous snapshot) are
/// checksummed only once and the result is reused — this avoids re-reading large
/// files that have not changed since the last snapshot.
pub fn generate_local(snapshot_dir: &Path) -> io::Result<()> {
    let mut entries: Vec<(String, String)> = Vec::new(); // (relative_path, hex_hash)
    let mut seen: HashMap<u64, String> = HashMap::new();

    collect_files(snapshot_dir, snapshot_dir, &mut entries, &mut seen)?;
    entries.sort_by(|a, b| a.0.cmp(&b.0));

    let manifest_path = snapshot_dir.join(MANIFEST_FILENAME);
    let mut out = File::create(&manifest_path)?;
    for (rel_path, hash) in &entries {
        writeln!(out, "{hash}  ./{rel_path}")?;
    }

    Ok(())
}

/// Recursively collects (relative_path, sha256_hex) pairs from `dir`.
fn collect_files(
    root: &Path,
    dir: &Path,
    out: &mut Vec<(String, String)>,
    seen: &mut HashMap<u64, String>,
) -> io::Result<()> {
    for entry in fs::read_dir(dir)? {
        let entry = entry?;
        let path = entry.path();
        let file_type = entry.file_type()?;

        // Never follow symlinks — they are directory structure, not file content.
        if file_type.is_symlink() {
            continue;
        }

        if file_type.is_dir() {
            collect_files(root, &path, out, seen)?;
        } else if file_type.is_file() {
            let rel: PathBuf = path
                .strip_prefix(root)
                .map_err(|e| io::Error::new(io::ErrorKind::Other, e))?
                .to_path_buf();
            let rel_str = rel.to_string_lossy().to_string();

            // The manifest must not include itself.
            if rel_str == MANIFEST_FILENAME {
                continue;
            }

            let hash = hash_or_dedup(&path, seen)?;
            out.push((rel_str, hash));
        }
    }
    Ok(())
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

/// Returns the SHA-256 hex digest of `path`, reusing a cached result when the
/// file shares an inode with a file already processed in this run (Unix only).
fn hash_or_dedup(path: &Path, seen: &mut HashMap<u64, String>) -> io::Result<String> {
    if let Some(ino) = inode_of(path) {
        if let Some(cached) = seen.get(&ino) {
            return Ok(cached.clone());
        }
        let hash = compute_hash(path)?;
        seen.insert(ino, hash.clone());
        Ok(hash)
    } else {
        compute_hash(path)
    }
}

/// Returns the inode number of `path` on Unix; `None` on other platforms.
fn inode_of(path: &Path) -> Option<u64> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;
        fs::metadata(path).ok().map(|m| m.ino())
    }
    #[cfg(not(unix))]
    {
        let _ = path;
        None
    }
}

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
