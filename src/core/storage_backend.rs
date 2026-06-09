use crate::core::checksum::{HashedEntry, NeedsHashing};
use crate::core::location::LocationKind;
use std::collections::HashMap;
use std::io::Write as _;
use std::path::PathBuf;
use std::process::{Command, Stdio};
use thiserror::Error;

const IN_PROGRESS_NAME: &str = "tmp";
const LATEST_NAME: &str = "latest";
const LOCK_NAME: &str = "backup.lock";

/// Suffix appended to a snapshot directory while it is being deleted.
///
/// Renaming to `<name>-expiring` before removal means that if the process is
/// killed mid-deletion the directory is no longer visible as a valid snapshot
/// (it won't match the `YYYY-MM-DD-HHMMSS` filter) and can be cleaned up on
/// the next run without any risk of confusing the retention logic.
const EXPIRING_SUFFIX: &str = "-expiring";

/// File name of the global checksum cache at the destination root.
const CHECKSUM_DB_NAME: &str = ".checksums.db";

/// File name of the per-snapshot checksum file inside each snapshot directory.
/// Chosen so users can run `cd <snapshot> && sha256sum -c sha256sum`.
const SNAPSHOT_SHA256_NAME: &str = "sha256sum";

/// Suffix appended to the in-flight tmp file used for atomic writes.
const TMP_SUFFIX: &str = ".tmp";

/// Errors from destination-side storage operations (local filesystem or SSH).
#[derive(Debug, Error)]
pub enum StorageBackendError {
    #[error("io error: {0}")]
    Io(#[from] std::io::Error),

    #[error("ssh command failed: {0}")]
    Ssh(String),

    /// An external command (`find`, `sha256sum`, `xargs`, …) failed or
    /// produced output we could not parse.
    #[error("external command failed: {0}")]
    Command(String),

    /// Returned when `backup.lock/` already exists at the destination, meaning
    /// another instance is running. The string is the full path to the lock directory.
    /// If no backup is active (e.g. a previous run crashed), remove it manually.
    #[error("another backup is already running; if not, remove '{0}' to unlock")]
    LockContended(String),

    /// Returned when a snapshot name does not match the expected `YYYY-MM-DD-HHMMSS`
    /// format. Rejects path traversal sequences such as `..` or absolute paths.
    #[error("invalid snapshot name '{0}': expected YYYY-MM-DD-HHMMSS format")]
    InvalidSnapshotName(String),
}

/// Validates that `name` is a safe snapshot name: exactly `YYYY-MM-DD-HHMMSS`
/// (17 characters, digits everywhere except dashes at positions 4, 7, 10).
///
/// This prevents path traversal (`../..`), shell injection, and any attempt to
/// operate on directories outside the backup root.
fn validate_snapshot_name(name: &str) -> Result<(), StorageBackendError> {
    let valid = name.len() == 17
        && name.chars().enumerate().all(|(i, c)| match i {
            4 | 7 | 10 => c == '-',
            _ => c.is_ascii_digit(),
        });
    if valid {
        Ok(())
    } else {
        Err(StorageBackendError::InvalidSnapshotName(name.to_string()))
    }
}

/// Same as [`validate_snapshot_name`] but also accepts the literal string
/// `"latest"`, which transparently resolves through the `latest` symlink to
/// the most recent snapshot.  Used by [`StorageBackend::verify_snapshot`]
/// so the user can run `justfolders verify` without naming a specific
/// timestamp.
fn validate_snapshot_name_or_latest(name: &str) -> Result<(), StorageBackendError> {
    if name == LATEST_NAME {
        Ok(())
    } else {
        validate_snapshot_name(name)
    }
}

/// Maps a `sha256sum -c` exit code to a [`VerifyOutcome`].
///
/// * `0` → every recorded hash matched.
/// * `1` → at least one mismatch (sha256sum already printed the offending
///   `FAILED` lines to stderr, so we just record the verdict here).
/// * anything else → the verification process itself misbehaved (signalled,
///   missing `sha256sum` binary, etc.) — surface as a real error so the
///   user gets a clear stack rather than a silent "everything's fine".
fn decode_sha256sum_check_status(
    code: Option<i32>,
) -> Result<VerifyOutcome, StorageBackendError> {
    match code {
        Some(0) => Ok(VerifyOutcome::Clean),
        Some(1) => Ok(VerifyOutcome::Mismatch),
        Some(other) => Err(StorageBackendError::Command(format!(
            "sha256sum -c exited unexpectedly with code {other}"
        ))),
        None => Err(StorageBackendError::Command(
            "sha256sum -c was terminated by a signal".to_string(),
        )),
    }
}

/// Abstracts destination-side filesystem operations for both local and remote destinations.
///
/// Implementations: [`LocalBackend`] for local paths, [`RemoteBackend`] for SSH destinations.
/// Use [`from_config`] to obtain the correct implementation for a given [`Config`].
pub trait StorageBackend {
    /// List all snapshot directories in the destination (format: YYYY-MM-DD-HHMMSS).
    fn list_snapshots(&self) -> Result<Vec<String>, StorageBackendError>;

    /// Delete a snapshot directory by name.
    fn delete_snapshot(&self, name: &str) -> Result<(), StorageBackendError>;

    /// Create or update the `latest` symlink pointing to the given snapshot name.
    fn update_latest_symlink(&self, snapshot_name: &str) -> Result<(), StorageBackendError>;

    /// Rename the in-progress `tmp` folder to the given snapshot name.
    ///
    /// This is called after rsync completes successfully to promote `<dest>/tmp`
    /// to a permanent timestamped snapshot folder.
    fn rename_in_progress(&self, snapshot_name: &str) -> Result<(), StorageBackendError>;

    /// Returns `true` if the `latest` symlink or folder exists in the destination.
    fn latest_exists(&self) -> Result<bool, StorageBackendError>;

    /// Creates a `backup.lock/` directory at the destination root to signal that a
    /// backup is in progress.
    ///
    /// Directory creation is atomic on all POSIX filesystems and NTFS, so the first
    /// concurrent caller wins and all subsequent ones get [`StorageBackendError::LockContended`].
    /// A `pid` file is written inside the directory for diagnostics.
    fn acquire_lock(&self) -> Result<(), StorageBackendError>;

    /// Removes the `backup.lock/` directory created by [`acquire_lock`].
    ///
    /// Called automatically by the RAII `BackupLock` guard; safe to call even if the
    /// lock directory has already been removed.
    fn release_lock(&self) -> Result<(), StorageBackendError>;

    // ── Checksum / verification I/O ──────────────────────────────────────────
    //
    // The methods below back the checksum subsystem (see [`crate::core::checksum`]).
    // They are deliberately phrased as a small handful of side-effecting
    // primitives so the orchestration code (and tests for it) can stay
    // backend-agnostic.

    /// List every regular file inside `<dest>/<snapshot_name>` together with
    /// its inode number.
    ///
    /// Paths in the result are **relative to the snapshot directory** — i.e.
    /// `etc/hosts`, not `/dest/2026-05-22-100000/etc/hosts`.  Implementations
    /// use `find -type f -printf '%i %P\0'` so the listing is safe for paths
    /// containing newlines or spaces.
    fn list_files_with_inodes(
        &self,
        snapshot_name: &str,
    ) -> Result<Vec<(u64, String)>, StorageBackendError>;

    /// Compute the SHA-256 of each file in `needs_hashing`, relative to
    /// `<dest>/<snapshot_name>`, and return one [`HashedEntry`] per input
    /// in the same order.
    ///
    /// Implementations shell out to `sha256sum` (locally via `xargs -0 sha256sum`,
    /// remotely via `ssh … xargs -0 sha256sum`).  The single shared mechanism
    /// keeps output parsing identical for both backends.
    fn hash_files(
        &self,
        snapshot_name: &str,
        needs_hashing: &[NeedsHashing],
    ) -> Result<Vec<HashedEntry>, StorageBackendError>;

    /// Read `<dest>/.checksums.db`.  Returns `Ok(None)` when the file does
    /// not exist (first backup, or it was deleted manually).
    fn read_checksum_db(&self) -> Result<Option<String>, StorageBackendError>;

    /// Atomically write `<dest>/.checksums.db`: writes to a `.tmp` sibling and
    /// renames into place.  An interrupted run leaves either the previous
    /// content or the new one, never a half-written file.
    fn write_checksum_db(&self, content: &str) -> Result<(), StorageBackendError>;

    /// Atomically write `<dest>/<snapshot_name>/sha256sum` (same tmp + rename
    /// scheme as [`write_checksum_db`]).
    fn write_snapshot_sha256(
        &self,
        snapshot_name: &str,
        content: &str,
    ) -> Result<(), StorageBackendError>;

    /// Read `<dest>/<snapshot_name>/sha256sum`.  Returns `Ok(None)` when the
    /// file is missing — e.g. for snapshots predating the checksum feature
    /// or for runs where checksumming was disabled.
    fn read_snapshot_sha256(
        &self,
        snapshot_name: &str,
    ) -> Result<Option<String>, StorageBackendError>;

    /// Verify a snapshot by running `sha256sum -c sha256sum` inside its
    /// directory.  Per-file `OK` / `FAILED` lines are emitted live to the
    /// current process's stdout / stderr so the user sees progress in real
    /// time — the [`VerifyOutcome`] returned just carries the verdict.
    ///
    /// `snapshot_name` may be the literal string `"latest"` to verify
    /// whichever snapshot the `latest` symlink currently points at;
    /// otherwise it must be a `YYYY-MM-DD-HHMMSS` timestamp.
    fn verify_snapshot(
        &self,
        snapshot_name: &str,
    ) -> Result<VerifyOutcome, StorageBackendError>;
}

/// Verdict of a [`StorageBackend::verify_snapshot`] run.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum VerifyOutcome {
    /// Every file in the snapshot matched its recorded SHA-256.
    Clean,
    /// At least one file mismatched (`sha256sum -c` exited with code 1).
    /// The per-file `FAILED` lines were already printed live by sha256sum.
    Mismatch,
    /// The snapshot has no `sha256sum` file — either it predates the
    /// checksum feature, or `checksum = false` was set in the config for
    /// the run that created it.  Not an error per se, but the snapshot
    /// cannot be verified.
    NoChecksumFile,
}

/// [`StorageBackend`] implementation for a local filesystem destination.
pub struct LocalBackend {
    path: PathBuf,
}

/// [`StorageBackend`] implementation for a remote destination accessed over SSH.
pub struct RemoteBackend {
    user: String,
    host: String,
    path: String,
    ssh_port: u16,
    id_rsa: Option<PathBuf>,
}

/// Constructs the appropriate [`StorageBackend`] for the destination specified in `config`.
///
/// Returns a [`LocalBackend`] for local destinations and a [`RemoteBackend`] for SSH destinations.
pub fn from_config(config: &crate::core::config::Config) -> Box<dyn StorageBackend> {
    match &config.dest.kind {
        LocationKind::Local(path) => Box::new(LocalBackend { path: path.clone() }),
        LocationKind::Remote {
            user, host, path, ..
        } => Box::new(RemoteBackend {
            user: user.clone(),
            host: host.clone(),
            path: path.clone(),
            ssh_port: config.port,
            id_rsa: config.id_rsa.clone(),
        }),
    }
}

impl RemoteBackend {
    /// Builds the base `ssh` command with the configured port and optional identity file.
    ///
    /// The host is formatted as `user@host` when a user is set, or just `host` when empty
    /// (allowing SSH config aliases to supply the user).
    fn ssh_command(&self) -> std::process::Command {
        let mut cmd = std::process::Command::new("ssh");
        cmd.arg("-p").arg(self.ssh_port.to_string());
        if let Some(key) = &self.id_rsa {
            cmd.arg("-i").arg(key);
        }
        if self.user.is_empty() {
            cmd.arg(&self.host);
        } else {
            cmd.arg(format!("{}@{}", self.user, self.host));
        }

        cmd
    }
}

impl StorageBackend for RemoteBackend {
    /// Lists snapshots by running `ls` on the remote destination over SSH.
    fn list_snapshots(&self) -> Result<Vec<String>, StorageBackendError> {
        let path = self.path.trim_end_matches('/');
        let cmd = format!(
            "ls -d '{path}/'[0-9][0-9][0-9][0-9]-* 2>/dev/null | xargs -I{{}} basename {{}}"
        );
        let output = self.ssh_command().arg(&cmd).output()?;

        if !output.status.success() {
            return Err(StorageBackendError::Ssh(
                String::from_utf8_lossy(&output.stderr).to_string(),
            ));
        }

        let mut snapshots: Vec<String> = String::from_utf8_lossy(&output.stdout)
            .lines()
            .map(|l| l.to_string())
            .filter(|l| !l.is_empty())
            .collect();

        snapshots.sort();
        Ok(snapshots)
    }

    /// Deletes the snapshot directory on the remote host using a two-step rename-then-remove.
    ///
    /// The directory is first renamed to `<name>-expiring` via `mv`, then
    /// removed with `rm -rf` — both over the same SSH connection.  If the
    /// process is terminated between the two steps the renamed directory is
    /// left on disk; it no longer matches the `YYYY-MM-DD-HHMMSS` filter so
    /// retention logic ignores it, and it can be cleaned up manually or by a
    /// future housekeeping pass.
    fn delete_snapshot(&self, name: &str) -> Result<(), StorageBackendError> {
        validate_snapshot_name(name)?;
        let expiring_name = format!("{name}{EXPIRING_SUFFIX}");
        let cmd = format!(
            "mv '{path}/{name}' '{path}/{expiring}' && rm -rf '{path}/{expiring}'",
            path = self.path,
            name = name,
            expiring = expiring_name,
        );
        let output = self.ssh_command().arg(&cmd).output()?;
        if !output.status.success() {
            return Err(StorageBackendError::Ssh(format!(
                "failed to delete snapshot '{}': {}",
                name,
                String::from_utf8_lossy(&output.stderr).trim()
            )));
        }
        Ok(())
    }

    /// Updates the `latest` symlink on the remote host via `ln -sfn` over SSH.
    fn update_latest_symlink(&self, snapshot_name: &str) -> Result<(), StorageBackendError> {
        validate_snapshot_name(snapshot_name)?;
        let cmd = format!(
            "ln -sfn '{}/{}' '{}/{}'",
            self.path, snapshot_name, self.path, LATEST_NAME
        );
        let status = self.ssh_command().arg(&cmd).status()?;
        if !status.success() {
            return Err(StorageBackendError::Ssh(
                "failed to update latest symlink".to_string(),
            ));
        }
        Ok(())
    }

    /// Renames `tmp` to the final snapshot name on the remote host via `mv` over SSH.
    fn rename_in_progress(&self, snapshot_name: &str) -> Result<(), StorageBackendError> {
        validate_snapshot_name(snapshot_name)?;
        let cmd = format!("mv '{}/{}' '{}/{}'", self.path, IN_PROGRESS_NAME, self.path, snapshot_name);
        let status = self.ssh_command().arg(&cmd).status()?;
        if !status.success() {
            return Err(StorageBackendError::Ssh(format!(
                "failed to rename tmp to {}",
                snapshot_name
            )));
        }
        Ok(())
    }

    /// Checks whether the `latest` entry exists on the remote host via `test -e` over SSH.
    fn latest_exists(&self) -> Result<bool, StorageBackendError> {
        let cmd = format!("test -e '{}/{}'", self.path, LATEST_NAME);
        let status = self.ssh_command().arg(&cmd).status()?;
        Ok(status.success())
    }

    /// Creates `<dest>/backup.lock/` on the remote host and writes the **local** PID into it.
    ///
    /// Using the local PID means liveness can be checked locally with [`is_process_alive`],
    /// avoiding complex remote shell logic. Two SSH calls are made: one to read any existing
    /// PID, and one to (re)create the lock directory with the current PID.
    fn acquire_lock(&self) -> Result<(), StorageBackendError> {
        let path = self.path.trim_end_matches('/');
        let lock = format!("{path}/{LOCK_NAME}");

        // Read the PID left by a previous run (empty output if no lock exists).
        let output = self
            .ssh_command()
            .arg(format!("cat '{lock}/pid' 2>/dev/null"))
            .output()?;
        let pid = String::from_utf8_lossy(&output.stdout)
            .trim()
            .parse::<u32>()
            .ok();

        if let Some(pid) = pid
            && is_process_alive(pid)
        {
            return Err(StorageBackendError::LockContended(format!(
                "{}:{}",
                self.host, lock
            )));
        }

        // No lock or stale lock — (re)create it with the local PID.
        let local_pid = std::process::id();
        let output = self
            .ssh_command()
            .arg(format!(
                "rm -rf '{lock}' && mkdir '{lock}' && echo {local_pid} > '{lock}/pid'"
            ))
            .output()?;
        if !output.status.success() {
            return Err(StorageBackendError::Ssh(format!(
                "failed to create lock at '{}:{}': {}",
                self.host,
                lock,
                String::from_utf8_lossy(&output.stderr).trim()
            )));
        }
        Ok(())
    }

    /// Removes `<dest>/backup.lock/` on the remote host via `rm -rf` over SSH.
    fn release_lock(&self) -> Result<(), StorageBackendError> {
        let path = self.path.trim_end_matches('/');
        let cmd = format!("rm -rf '{path}/{LOCK_NAME}'");
        let status = self.ssh_command().arg(&cmd).status()?;
        if !status.success() {
            return Err(StorageBackendError::Ssh(
                "failed to release backup lock".to_string(),
            ));
        }
        Ok(())
    }

    // ── Checksum / verification I/O ──────────────────────────────────────────
    //
    // All operations run on the **remote** host over SSH:
    //   * Listing & hashing → `find` / `xargs sha256sum` on the remote.
    //   * Reading & writing artifacts → `cat` / shell redirection over SSH.
    //
    // Single-quoting around paths is safe because `RemoteBackend.path` was
    // validated at config-load time to not contain a single quote, and
    // `snapshot_name` is always checked by [`validate_snapshot_name`] before
    // any command is built.

    /// Runs `find -type f -printf '%i %P\0'` on the remote and parses the
    /// returned NUL-stream — identical record format to the local backend so
    /// the same parser handles both.
    fn list_files_with_inodes(
        &self,
        snapshot_name: &str,
    ) -> Result<Vec<(u64, String)>, StorageBackendError> {
        validate_snapshot_name(snapshot_name)?;
        let snap_path = self.snapshot_path(snapshot_name);

        // The `\0` inside the single-quoted shell argument reaches `find`
        // literally; `find` then emits real NUL bytes between records.
        let cmd = format!("find '{snap_path}' -type f -printf '%i %P\\0'");
        let output = self.ssh_command().arg(&cmd).output()?;

        if !output.status.success() {
            return Err(StorageBackendError::Ssh(format!(
                "remote find failed for {snap_path}: {}",
                String::from_utf8_lossy(&output.stderr).trim()
            )));
        }

        parse_find_inodes(&output.stdout)
    }

    /// Pipes the NUL-separated rel-path list to
    /// `cd <snap> && xargs -0 -P <ncpus> -n <batch> sha256sum` on the
    /// remote, then parses the standard sha256sum output (same parser the
    /// local backend uses — output format is identical).
    ///
    /// Parallelism is computed by the *remote* shell so it reflects the
    /// remote host's CPU count (and its cgroup limits when `nproc` is
    /// cgroup-aware), not ours:
    ///   * `NPROC = nproc 2>/dev/null || echo 1` — falls back to serial on
    ///     hosts where `nproc` is missing (BSDs, stripped Alpine), so we
    ///     never regress versus the pre-parallel command.
    ///   * `BATCH = ceil(total / (4 * NPROC))` — same heuristic as
    ///     [`hash_parallelism_params`], but evaluated in POSIX shell so it
    ///     sees the right `NPROC`.  `total` is interpolated as a literal
    ///     because it's only known locally (`needs_hashing.len()`).
    ///   * `-n BATCH` is required for `-P` to actually fan out; without it
    ///     xargs packs the whole stdin stream into a single sha256sum
    ///     invocation and `-P` has nothing to parallelize.
    fn hash_files(
        &self,
        snapshot_name: &str,
        needs_hashing: &[NeedsHashing],
    ) -> Result<Vec<HashedEntry>, StorageBackendError> {
        validate_snapshot_name(snapshot_name)?;
        if needs_hashing.is_empty() {
            return Ok(Vec::new());
        }
        let snap_path = self.snapshot_path(snapshot_name);
        let total = needs_hashing.len();

        let mut cmd = self.ssh_command();
        // The `cd` makes sha256sum print paths relative to the snapshot
        // root, which is the form our cache expects.  See the doc comment
        // above for why NPROC/BATCH are computed on the remote.
        cmd.arg(format!(
            "cd '{snap_path}' && \
             NPROC=$(nproc 2>/dev/null || echo 1) && \
             BATCH=$(( ({total} + 4 * NPROC - 1) / (4 * NPROC) )) && \
             xargs -0 --no-run-if-empty -P \"$NPROC\" -n \"$BATCH\" sha256sum"
        ));

        let by_path = run_sha256sum_via_xargs(cmd, needs_hashing)?;
        zip_hashes(needs_hashing, by_path)
    }

    /// Reads `<dest>/.checksums.db` via SSH.  Distinguishes "file does not
    /// exist" from real errors by using a dedicated exit code (42) inside
    /// the remote shell script — `cat` alone would conflate both cases.
    fn read_checksum_db(&self) -> Result<Option<String>, StorageBackendError> {
        let db_path = self.dest_file_path(CHECKSUM_DB_NAME);
        self.read_remote_file_optional(&db_path)
    }

    /// Atomic remote write: stream the content into a `.tmp` sibling, then
    /// `mv` it into place.  POSIX `mv` between paths on the same filesystem
    /// is atomic, so an interrupted run leaves either the old file or the
    /// new one — never a half-written one.
    fn write_checksum_db(&self, content: &str) -> Result<(), StorageBackendError> {
        let final_path = self.dest_file_path(CHECKSUM_DB_NAME);
        let tmp_path = format!("{final_path}{TMP_SUFFIX}");
        self.write_remote_file_atomic(&tmp_path, &final_path, content.as_bytes())
    }

    /// Same atomic write pattern as [`Self::write_checksum_db`], scoped to a
    /// snapshot directory.
    fn write_snapshot_sha256(
        &self,
        snapshot_name: &str,
        content: &str,
    ) -> Result<(), StorageBackendError> {
        validate_snapshot_name(snapshot_name)?;
        let snap_path = self.snapshot_path(snapshot_name);
        let final_path = format!("{snap_path}/{SNAPSHOT_SHA256_NAME}");
        let tmp_path = format!("{final_path}{TMP_SUFFIX}");
        self.write_remote_file_atomic(&tmp_path, &final_path, content.as_bytes())
    }

    /// Reads `<dest>/<snap>/sha256sum`.  Returns `Ok(None)` for snapshots
    /// that predate the checksum feature (or were created with
    /// `checksum = false` — see step 6).
    fn read_snapshot_sha256(
        &self,
        snapshot_name: &str,
    ) -> Result<Option<String>, StorageBackendError> {
        validate_snapshot_name(snapshot_name)?;
        let snap_path = self.snapshot_path(snapshot_name);
        let file_path = format!("{snap_path}/{SNAPSHOT_SHA256_NAME}");
        self.read_remote_file_optional(&file_path)
    }

    /// Runs `cd '<snap>' && sha256sum -c --quiet sha256sum` on the remote
    /// host.  stdout/stderr stream live over SSH so the user sees any
    /// `<file>: FAILED` lines as they appear; `--quiet` suppresses the
    /// per-file `OK` chatter so the local view matches the local backend.
    ///
    /// Existence of the per-snapshot file is probed first so we can return
    /// [`VerifyOutcome::NoChecksumFile`] cleanly rather than relying on the
    /// remote `sha256sum -c` error message.
    fn verify_snapshot(
        &self,
        snapshot_name: &str,
    ) -> Result<VerifyOutcome, StorageBackendError> {
        validate_snapshot_name_or_latest(snapshot_name)?;
        let snap_path = self.snapshot_path(snapshot_name);

        // Probe existence first.  Same exit-code-42 trick we use for
        // `read_remote_file_optional`, just without the actual cat payload.
        let probe = format!(
            "if [ -f '{snap_path}/{SNAPSHOT_SHA256_NAME}' ]; then exit 0; else exit 42; fi"
        );
        let probe_status = self.ssh_command().arg(&probe).status()?;
        match probe_status.code() {
            Some(0) => {}
            Some(42) => return Ok(VerifyOutcome::NoChecksumFile),
            _ => {
                return Err(StorageBackendError::Ssh(format!(
                    "failed to probe '{snap_path}/{SNAPSHOT_SHA256_NAME}'"
                )))
            }
        }

        let cmd = format!("cd '{snap_path}' && sha256sum -c --quiet {SNAPSHOT_SHA256_NAME}");
        let status = self.ssh_command().arg(&cmd).status()?;
        decode_sha256sum_check_status(status.code())
    }
}

// ── RemoteBackend helpers (paths and small SSH wrappers) ────────────────────

impl RemoteBackend {
    /// Builds the absolute path of `<dest>/<snapshot_name>` on the remote
    /// host (trailing slashes on `self.path` are normalized away).
    fn snapshot_path(&self, snapshot_name: &str) -> String {
        format!("{}/{}", self.path.trim_end_matches('/'), snapshot_name)
    }

    /// Absolute path of a file directly inside the destination root.
    fn dest_file_path(&self, file_name: &str) -> String {
        format!("{}/{}", self.path.trim_end_matches('/'), file_name)
    }

    /// Reads a remote file via SSH.  Exit code 42 (chosen because nothing
    /// in `cat` uses it) signals "file does not exist"; anything else
    /// non-zero is a real error and the stderr is surfaced.
    fn read_remote_file_optional(
        &self,
        remote_path: &str,
    ) -> Result<Option<String>, StorageBackendError> {
        let cmd =
            format!("if [ -f '{remote_path}' ]; then cat '{remote_path}'; else exit 42; fi");
        let output = self.ssh_command().arg(&cmd).output()?;
        match output.status.code() {
            Some(0) => Ok(Some(String::from_utf8_lossy(&output.stdout).into_owned())),
            Some(42) => Ok(None),
            _ => Err(StorageBackendError::Ssh(format!(
                "failed to read remote file '{remote_path}': {}",
                String::from_utf8_lossy(&output.stderr).trim()
            ))),
        }
    }

    /// Atomic remote write helper used by both `write_checksum_db` and
    /// `write_snapshot_sha256`: stream `content` into `tmp_path` via the
    /// remote shell, then `mv` to `final_path`.  On failure the tmp file
    /// is left in place — the next backup will overwrite it.
    fn write_remote_file_atomic(
        &self,
        tmp_path: &str,
        final_path: &str,
        content: &[u8],
    ) -> Result<(), StorageBackendError> {
        let cmd = format!("cat > '{tmp_path}' && mv '{tmp_path}' '{final_path}'");
        let mut child = self
            .ssh_command()
            .arg(&cmd)
            .stdin(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()?;

        // Closes on scope exit, which signals EOF to the remote `cat` and
        // lets the chained `mv` run.
        {
            let stdin = child.stdin.as_mut().ok_or_else(|| {
                StorageBackendError::Ssh("ssh process has no stdin".to_string())
            })?;
            stdin.write_all(content)?;
        }

        let output = child.wait_with_output()?;
        if !output.status.success() {
            return Err(StorageBackendError::Ssh(format!(
                "remote write to '{final_path}' failed: {}",
                String::from_utf8_lossy(&output.stderr).trim()
            )));
        }
        Ok(())
    }
}

/// Returns `true` if the process with the given PID is still running.
///
/// On Linux every live process has a `/proc/<pid>` directory that disappears
/// Returns `true` if the given process is still running on the **local** machine.
///
/// On Linux `/proc/<pid>` disappears the moment the process exits.
/// On non-Linux we cannot reliably check without adding a C dependency, so we
/// conservatively report the process as *alive* — this means stale locks on
/// non-Linux platforms must be removed manually.  That is the safe default
/// (never silently clobber a concurrent backup).
fn is_process_alive(pid: u32) -> bool {
    #[cfg(target_os = "linux")]
    {
        std::path::Path::new(&format!("/proc/{pid}")).exists()
    }
    #[cfg(not(target_os = "linux"))]
    {
        let _ = pid;
        // Conservative: assume alive so we never auto-clear on unknown platforms.
        true
    }
}


impl StorageBackend for LocalBackend {
    /// Lists snapshots by reading the local destination directory.
    fn list_snapshots(&self) -> Result<Vec<String>, StorageBackendError> {
        let mut snapshots = vec![];
        for entry in std::fs::read_dir(&self.path)? {
            let entry = entry?;
            let name = entry.file_name().to_string_lossy().to_string();
            // match YYYY-MM-DD-HHMMSS format
            if name.len() == 17 && name.chars().nth(4) == Some('-') {
                snapshots.push(name);
            }
        }
        snapshots.sort();
        Ok(snapshots)
    }

    /// Deletes a snapshot directory using a two-step rename-then-remove.
    ///
    /// The directory is first renamed to `<name>-expiring` before it is
    /// removed.  If the process is terminated between the two steps the
    /// renamed directory is simply left on disk; it no longer matches the
    /// `YYYY-MM-DD-HHMMSS` filter so retention logic ignores it, and it can
    /// be cleaned up manually or by a future housekeeping pass.
    fn delete_snapshot(&self, name: &str) -> Result<(), StorageBackendError> {
        validate_snapshot_name(name)?;
        let path = self.path.join(name);
        // Defense-in-depth: ensure the resolved path is still under self.path.
        // validate_snapshot_name already forbids "..", but this is an extra guard.
        if !path.starts_with(&self.path) {
            return Err(StorageBackendError::InvalidSnapshotName(name.to_string()));
        }
        let expiring_path = self.path.join(format!("{name}{EXPIRING_SUFFIX}"));
        std::fs::rename(&path, &expiring_path)?;
        std::fs::remove_dir_all(&expiring_path)?;
        Ok(())
    }

    /// Removes any existing `latest` entry and creates a new directory symlink pointing
    /// to the given snapshot.
    fn update_latest_symlink(&self, snapshot_name: &str) -> Result<(), StorageBackendError> {
        let latest = self.path.join(LATEST_NAME);
        let target = self.path.join(snapshot_name);

        if latest.exists() || latest.is_symlink() {
            std::fs::remove_file(&latest)?;
        }

        #[cfg(unix)]
        std::os::unix::fs::symlink(&target, &latest)?;
        #[cfg(windows)]
        std::os::windows::fs::symlink_dir(&target, &latest)?;

        Ok(())
    }

    /// Renames `tmp` to the final snapshot name using [`std::fs::rename`].
    fn rename_in_progress(&self, snapshot_name: &str) -> Result<(), StorageBackendError> {
        let from = self.path.join(IN_PROGRESS_NAME);
        let to = self.path.join(snapshot_name);
        std::fs::rename(&from, &to)?;
        Ok(())
    }

    /// Returns `true` if a `latest` entry (symlink or directory) exists in the destination.
    fn latest_exists(&self) -> Result<bool, StorageBackendError> {
        let latest = self.path.join(LATEST_NAME);
        Ok(latest.exists() || latest.is_symlink())
    }

    /// Creates `<dest>/backup.lock/` and writes the current PID inside it.
    ///
    /// If the directory already exists, the PID file is read and checked with
    /// [`is_process_alive`]. A stale lock (process no longer alive) is removed
    /// automatically before the new lock is created. A live lock returns
    /// [`StorageBackendError::LockContended`].
    fn acquire_lock(&self) -> Result<(), StorageBackendError> {
        let lock = self.path.join(LOCK_NAME);

        if lock.exists() {
            let pid_str = std::fs::read_to_string(lock.join("pid")).unwrap_or_default();
            let pid = pid_str.trim().parse::<u32>().ok();
            let still_alive = pid.map(is_process_alive).unwrap_or(false);

            if still_alive {
                return Err(StorageBackendError::LockContended(
                    lock.display().to_string(),
                ));
            }
            // Stale lock — the process is gone; remove it and proceed.
            std::fs::remove_dir_all(&lock)?;
        }

        std::fs::create_dir(&lock).map_err(|e| {
            if e.kind() == std::io::ErrorKind::AlreadyExists {
                StorageBackendError::LockContended(lock.display().to_string())
            } else {
                StorageBackendError::Io(e)
            }
        })?;
        std::fs::write(lock.join("pid"), std::process::id().to_string())?;
        Ok(())
    }

    /// Removes `<dest>/backup.lock/` to release the lock.
    fn release_lock(&self) -> Result<(), StorageBackendError> {
        let lock = self.path.join(LOCK_NAME);
        if lock.exists() {
            std::fs::remove_dir_all(&lock)?;
        }
        Ok(())
    }

    // ── Checksum / verification I/O ──────────────────────────────────────────

    /// Lists files via local `find -type f -printf '%i %P\0'`.
    ///
    /// `%P` prints the path **relative to the starting point**, which is the
    /// representation the checksum subsystem expects everywhere.  Records are
    /// NUL-separated to safely handle filenames containing newlines or spaces.
    fn list_files_with_inodes(
        &self,
        snapshot_name: &str,
    ) -> Result<Vec<(u64, String)>, StorageBackendError> {
        validate_snapshot_name(snapshot_name)?;
        let snap_root = self.path.join(snapshot_name);

        // The "%i %P\0" format must reach `find` as the 8-character literal
        // `%i %P\0`, *not* a Rust string containing an actual NUL byte —
        // `Command::arg` rejects embedded NULs.  `find` then interprets the
        // backslash-zero pair as a request to emit a literal NUL between
        // records, which is what we need for safe parsing.
        let output = Command::new("find")
            .arg(&snap_root)
            .arg("-type")
            .arg("f")
            .arg("-printf")
            .arg("%i %P\\0")
            .output()?;

        if !output.status.success() {
            return Err(StorageBackendError::Command(format!(
                "find failed for {}: {}",
                snap_root.display(),
                String::from_utf8_lossy(&output.stderr).trim()
            )));
        }

        parse_find_inodes(&output.stdout)
    }

    /// Hashes files locally by piping the path list to
    /// `xargs -0 -P <ncpus> -n <batch> sha256sum`, run with the snapshot
    /// directory as the working directory.  The input list and the output
    /// are both NUL-/newline-streamed so memory pressure stays flat with
    /// the number of files.
    ///
    /// Parallelism: `-P <ncpus>` lets xargs run up to one sha256sum process
    /// per logical CPU; `-n <batch>` is required for `-P` to actually fan
    /// out (otherwise xargs packs everything into a single invocation).
    /// The batch size targets ~4 batches per worker so faster workers can
    /// keep picking up work when file sizes vary, avoiding long tails.
    ///
    /// Output ordering is unaffected: each sha256sum line stays under
    /// `PIPE_BUF`, so concurrent writes to the shared pipe remain atomic at
    /// line boundaries, and [`zip_hashes`] re-attaches results in caller
    /// order via the `rel_path → hash` map.
    fn hash_files(
        &self,
        snapshot_name: &str,
        needs_hashing: &[NeedsHashing],
    ) -> Result<Vec<HashedEntry>, StorageBackendError> {
        validate_snapshot_name(snapshot_name)?;
        if needs_hashing.is_empty() {
            return Ok(Vec::new());
        }
        let snap_root = self.path.join(snapshot_name);

        let (parallelism, batch_size) = hash_parallelism_params(needs_hashing.len());

        let mut cmd = Command::new("xargs");
        cmd.arg("-0")
            .arg("--no-run-if-empty")
            .arg("-P")
            .arg(parallelism.to_string())
            .arg("-n")
            .arg(batch_size.to_string())
            .arg("sha256sum");
        cmd.current_dir(&snap_root);

        let by_path = run_sha256sum_via_xargs(cmd, needs_hashing)?;
        zip_hashes(needs_hashing, by_path)
    }

    /// Reads `<dest>/.checksums.db` into a `String`, or returns `Ok(None)` if
    /// the file does not exist (first backup, or it was removed manually).
    fn read_checksum_db(&self) -> Result<Option<String>, StorageBackendError> {
        let path = self.path.join(CHECKSUM_DB_NAME);
        match std::fs::read_to_string(&path) {
            Ok(content) => Ok(Some(content)),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
            Err(e) => Err(e.into()),
        }
    }

    /// Atomic write: `.checksums.db.tmp` is written first then renamed over
    /// `.checksums.db`.  On POSIX `rename(2)` is atomic on the same filesystem,
    /// so a crash mid-write leaves the previous file untouched.
    fn write_checksum_db(&self, content: &str) -> Result<(), StorageBackendError> {
        let final_path = self.path.join(CHECKSUM_DB_NAME);
        let tmp_path = self.path.join(format!("{CHECKSUM_DB_NAME}{TMP_SUFFIX}"));
        std::fs::write(&tmp_path, content)?;
        std::fs::rename(&tmp_path, &final_path)?;
        Ok(())
    }

    /// Same tmp-file + rename scheme as [`Self::write_checksum_db`], scoped
    /// to a snapshot directory.
    fn write_snapshot_sha256(
        &self,
        snapshot_name: &str,
        content: &str,
    ) -> Result<(), StorageBackendError> {
        validate_snapshot_name(snapshot_name)?;
        let snap_root = self.path.join(snapshot_name);
        let final_path = snap_root.join(SNAPSHOT_SHA256_NAME);
        let tmp_path = snap_root.join(format!("{SNAPSHOT_SHA256_NAME}{TMP_SUFFIX}"));
        std::fs::write(&tmp_path, content)?;
        std::fs::rename(&tmp_path, &final_path)?;
        Ok(())
    }

    /// Reads the per-snapshot `sha256sum` file, or `Ok(None)` if it is
    /// missing (snapshot predates the feature or was created with
    /// `checksum = false`).
    fn read_snapshot_sha256(
        &self,
        snapshot_name: &str,
    ) -> Result<Option<String>, StorageBackendError> {
        validate_snapshot_name(snapshot_name)?;
        let path = self.path.join(snapshot_name).join(SNAPSHOT_SHA256_NAME);
        match std::fs::read_to_string(&path) {
            Ok(content) => Ok(Some(content)),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
            Err(e) => Err(e.into()),
        }
    }

    /// Runs `sha256sum -c --quiet sha256sum` inside the snapshot directory.
    /// stdout/stderr are inherited from the current process so any per-file
    /// `FAILED` lines stream live; `--quiet` suppresses the noisy `OK`
    /// lines so the user sees output only when something is actually wrong.
    /// The final verdict is captured in the return value.
    fn verify_snapshot(
        &self,
        snapshot_name: &str,
    ) -> Result<VerifyOutcome, StorageBackendError> {
        validate_snapshot_name_or_latest(snapshot_name)?;
        let snap_root = self.path.join(snapshot_name);
        if !snap_root.join(SNAPSHOT_SHA256_NAME).exists() {
            return Ok(VerifyOutcome::NoChecksumFile);
        }

        let status = Command::new("sha256sum")
            .arg("-c")
            .arg("--quiet")
            .arg(SNAPSHOT_SHA256_NAME)
            .current_dir(&snap_root)
            .status()?;

        decode_sha256sum_check_status(status.code())
    }
}

// ── Helpers shared by both backends ─────────────────────────────────────────

/// Parses the NUL-separated output of `find -printf '%i %P\0'` into
/// `(inode, relative_path)` pairs.
///
/// Each record is `"<inode> <rel_path>"` where `<rel_path>` is everything
/// after the first space.  Non-UTF-8 paths fall back to `String::from_utf8_lossy`
/// — a path with a replacement character will still be hashed correctly
/// (the bytes on disk are unchanged) but it will miss the cache on the
/// following backup and be re-hashed.
fn parse_find_inodes(bytes: &[u8]) -> Result<Vec<(u64, String)>, StorageBackendError> {
    let mut out = Vec::new();
    for record in bytes.split(|&b| b == 0) {
        if record.is_empty() {
            continue;
        }
        let space_pos = record.iter().position(|&b| b == b' ').ok_or_else(|| {
            StorageBackendError::Command(format!(
                "malformed find record (no space): {}",
                String::from_utf8_lossy(record)
            ))
        })?;
        let inode_bytes = &record[..space_pos];
        let path_bytes = &record[space_pos + 1..];

        let inode_str = std::str::from_utf8(inode_bytes).map_err(|_| {
            StorageBackendError::Command(
                "non-utf8 inode in find record (this should be impossible)".to_string(),
            )
        })?;
        let inode: u64 = inode_str.parse().map_err(|_| {
            StorageBackendError::Command(format!("invalid inode '{inode_str}' from find"))
        })?;

        let path = String::from_utf8_lossy(path_bytes).into_owned();
        // Defensive: an empty relative path means the search root itself
        // (find emits "%i\0" for the root with %P).  We never want that.
        if !path.is_empty() {
            out.push((inode, path));
        }
    }
    Ok(out)
}

/// Returns `(parallelism, batch_size)` for the local `xargs -P … -n …`
/// invocation that drives `sha256sum`.
///
/// * `parallelism` = number of logical CPUs reported by the OS, falling
///   back to 1 when the platform refuses to answer.
/// * `batch_size`  = `ceil(total / (parallelism * 4))`, clamped to ≥ 1.
///   Aiming for ~4 batches per worker keeps faster workers busy when file
///   sizes vary; with too few batches a single slow file would stall an
///   otherwise idle CPU at the tail of the run.
///
/// `total` must be > 0; callers gate the empty case before invoking
/// sha256sum at all.
fn hash_parallelism_params(total: usize) -> (usize, usize) {
    let parallelism = std::thread::available_parallelism()
        .map(|n| n.get())
        .unwrap_or(1);
    let target_batches = parallelism.saturating_mul(4).max(1);
    let batch_size = total.div_ceil(target_batches).max(1);
    (parallelism, batch_size)
}

/// Spawns the supplied `cmd` (a fully configured `xargs -0 sha256sum`
/// invocation — local or wrapped in `ssh …`), feeds the NUL-separated
/// rel-path list over stdin, and returns a map of `rel_path → hash`.
///
/// Returning the map by path lets the caller re-attach the inode that came
/// from `needs_hashing` without depending on input ordering being preserved
/// by `xargs`.
///
/// The caller is responsible for putting the command in the right working
/// directory:
///   * Local backend  → `cmd.current_dir(snapshot_root)`.
///   * Remote backend → the shell command run via ssh starts with
///     `cd '<snapshot_root>' && xargs …`.
fn run_sha256sum_via_xargs(
    mut cmd: Command,
    needs_hashing: &[NeedsHashing],
) -> Result<HashMap<String, String>, StorageBackendError> {
    cmd.stdin(Stdio::piped());
    cmd.stdout(Stdio::piped());
    cmd.stderr(Stdio::piped());

    let mut child = cmd.spawn().map_err(|e| {
        StorageBackendError::Command(format!("failed to spawn sha256sum pipeline: {e}"))
    })?;

    // IMPORTANT: drain stdout/stderr concurrently with feeding stdin.
    //
    // If we feed all paths first and only then read stdout, the pipeline can
    // deadlock when `sha256sum` fills the stdout pipe:
    // `sha256sum` blocks in pipe_write → `xargs` stops reading stdin → we block
    // trying to write more paths.
    let mut child_stdout = child.stdout.take().ok_or_else(|| {
        StorageBackendError::Command("sha256sum pipeline has no stdout".to_string())
    })?;
    let mut child_stderr = child.stderr.take().ok_or_else(|| {
        StorageBackendError::Command("sha256sum pipeline has no stderr".to_string())
    })?;

    let stdout_handle = std::thread::spawn(move || {
        let mut buf = Vec::new();
        let res = std::io::Read::read_to_end(&mut child_stdout, &mut buf);
        (res, buf)
    });
    let stderr_handle = std::thread::spawn(move || {
        let mut buf = Vec::new();
        let res = std::io::Read::read_to_end(&mut child_stderr, &mut buf);
        (res, buf)
    });

    // Pipe the NUL-separated path list to xargs. Dropping stdin closes it so
    // xargs sees EOF and runs sha256sum.
    {
        let mut stdin = child.stdin.take().ok_or_else(|| {
            StorageBackendError::Command("sha256sum pipeline has no stdin".to_string())
        })?;
        for nh in needs_hashing {
            stdin.write_all(nh.rel_path.as_bytes())?;
            stdin.write_all(b"\0")?;
        }
    }

    let status = child.wait()?;

    let (stdout_res, stdout) = stdout_handle
        .join()
        .map_err(|_| StorageBackendError::Command("sha256sum stdout reader panicked".to_string()))?;
    let (stderr_res, stderr) = stderr_handle
        .join()
        .map_err(|_| StorageBackendError::Command("sha256sum stderr reader panicked".to_string()))?;

    stdout_res?;
    stderr_res?;

    if !status.success() {
        return Err(StorageBackendError::Command(format!(
            "sha256sum pipeline exited {}: {}",
            status,
            String::from_utf8_lossy(&stderr).trim()
        )));
    }

    parse_sha256sum_output(&stdout)
}

/// Parses the standard GNU coreutils `sha256sum` output format into a
/// `rel_path → hash` map.
///
/// Accepts both text mode (`<hash>  <path>`) and binary mode
/// (`<hash> *<path>`).  Lines whose path needs escaping (paths containing
/// `\` or `\n`) are prefixed by `\` and the path uses `\\` and `\n` escapes;
/// the decoder reverses both.
fn parse_sha256sum_output(stdout: &[u8]) -> Result<HashMap<String, String>, StorageBackendError> {
    let text = std::str::from_utf8(stdout).map_err(|_| {
        StorageBackendError::Command("sha256sum produced non-utf8 output".to_string())
    })?;

    let mut out = HashMap::new();
    for line in text.lines() {
        if line.is_empty() {
            continue;
        }
        let (is_escaped, body) = match line.strip_prefix('\\') {
            Some(rest) => (true, rest),
            None => (false, line),
        };

        // The body is `<64-hex><sep><path>` where <sep> is `"  "` (text mode)
        // or `" *"` (binary mode).  We require at least 64 + 2 chars.
        if body.len() < 66 {
            return Err(StorageBackendError::Command(format!(
                "malformed sha256sum line: '{line}'"
            )));
        }
        let (hash, sep_and_path) = body.split_at(64);
        if !hash.chars().all(|c| c.is_ascii_hexdigit()) {
            return Err(StorageBackendError::Command(format!(
                "invalid hex hash in sha256sum line: '{line}'"
            )));
        }
        let path_part = if let Some(rest) = sep_and_path.strip_prefix("  ") {
            rest
        } else if let Some(rest) = sep_and_path.strip_prefix(" *") {
            rest
        } else {
            return Err(StorageBackendError::Command(format!(
                "unexpected separator in sha256sum line: '{line}'"
            )));
        };

        let path = if is_escaped {
            decode_sha256sum_path(path_part)
        } else {
            path_part.to_string()
        };
        out.insert(path, hash.to_string());
    }
    Ok(out)
}

/// Reverses the GNU coreutils escape scheme used when a filename contains
/// `\` or a newline: inside the path, `\\` → `\` and `\n` → LF.  Any unknown
/// `\x` escape is passed through verbatim, matching how coreutils itself
/// treats unrecognised escapes.
fn decode_sha256sum_path(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut chars = s.chars();
    while let Some(c) = chars.next() {
        if c == '\\' {
            match chars.next() {
                Some('\\') => out.push('\\'),
                Some('n') => out.push('\n'),
                Some(other) => {
                    out.push('\\');
                    out.push(other);
                }
                None => out.push('\\'),
            }
        } else {
            out.push(c);
        }
    }
    out
}

/// Joins the inode-bearing input list with the path→hash map returned by
/// [`run_sha256sum_via_xargs`] to produce one [`HashedEntry`] per input,
/// preserving the caller's order.
fn zip_hashes(
    needs_hashing: &[NeedsHashing],
    mut by_path: HashMap<String, String>,
) -> Result<Vec<HashedEntry>, StorageBackendError> {
    let mut out = Vec::with_capacity(needs_hashing.len());
    for nh in needs_hashing {
        let hash = by_path.remove(&nh.rel_path).ok_or_else(|| {
            StorageBackendError::Command(format!(
                "sha256sum did not return a hash for '{}'",
                nh.rel_path
            ))
        })?;
        out.push(HashedEntry {
            rel_path: nh.rel_path.clone(),
            inode: nh.inode,
            hash,
        });
    }
    Ok(out)
}
