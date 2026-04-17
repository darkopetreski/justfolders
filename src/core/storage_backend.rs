use crate::core::location::LocationKind;
use crate::core::manifest::{self, VerifyReport};
use std::path::PathBuf;
use thiserror::Error;

const IN_PROGRESS_NAME: &str = "tmp";
const LATEST_NAME: &str = "latest";
const LOCK_NAME: &str = "backup.lock";

/// Errors from destination-side storage operations (local filesystem or SSH).
#[derive(Debug, Error)]
pub enum StorageBackendError {
    #[error("snapshot directory not found: {0}")]
    MissingSnapshot(String),

    #[error("io error: {0}")]
    Io(#[from] std::io::Error),

    #[error("ssh command failed: {0}")]
    Ssh(String),

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

    /// Generates a `SHA256SUMS` manifest file inside the given snapshot directory.
    ///
    /// Called automatically at the end of every successful backup run. The manifest
    /// is the input for [`verify_snapshot`] and enables offline bit-rot detection.
    fn generate_manifest(&self, snapshot_name: &str) -> Result<(), StorageBackendError>;

    /// Verifies a snapshot by re-checksumming every file against its `SHA256SUMS` manifest.
    ///
    /// Returns a [`VerifyReport`] listing any mismatches or missing files.
    /// Returns [`StorageBackendError::MissingSnapshot`] if no manifest exists in the snapshot
    /// (e.g. the snapshot was created before manifest generation was introduced).
    fn verify_snapshot(&self, snapshot_name: &str) -> Result<VerifyReport, StorageBackendError>;
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

    /// Deletes the snapshot directory on the remote host via `rm -rf` over SSH.
    fn delete_snapshot(&self, name: &str) -> Result<(), StorageBackendError> {
        validate_snapshot_name(name)?;
        let cmd = format!("rm -rf '{}/{}'", self.path, name);
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

        if let Some(pid) = pid {
            if is_process_alive(pid) {
                return Err(StorageBackendError::LockContended(format!(
                    "{}:{}",
                    self.host, lock
                )));
            }
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

    /// Generates a `SHA256SUMS` manifest on the remote host via SSH.
    ///
    /// Uses `find | sort | xargs sha256sum` so the output is deterministic
    /// regardless of filesystem directory ordering.
    fn generate_manifest(&self, snapshot_name: &str) -> Result<(), StorageBackendError> {
        validate_snapshot_name(snapshot_name)?;
        let path = format!("{}/{}", self.path.trim_end_matches('/'), snapshot_name);
        // -P: never follow symlinks; without it a symlink loop on the remote
        // could cause find to walk outside the snapshot directory.
        let cmd = format!(
            "cd '{path}' && find -P . -type f ! -name '{mf}' -print0 \
             | sort -z | xargs -0 sha256sum > '{mf}'",
            mf = manifest::MANIFEST_FILENAME,
        );
        let output = self.ssh_command().arg(&cmd).output()?;
        if !output.status.success() {
            return Err(StorageBackendError::Ssh(format!(
                "failed to generate manifest for snapshot '{snapshot_name}': {}",
                String::from_utf8_lossy(&output.stderr).trim()
            )));
        }
        Ok(())
    }

    /// Verifies the remote snapshot by running `sha256sum --check` on the remote host.
    ///
    /// Parses the output to build a [`VerifyReport`] without transferring any file data.
    fn verify_snapshot(&self, snapshot_name: &str) -> Result<VerifyReport, StorageBackendError> {
        validate_snapshot_name(snapshot_name)?;
        let path = format!("{}/{}", self.path.trim_end_matches('/'), snapshot_name);

        // Check the manifest exists before attempting verification.
        let check = format!(
            "test -f '{}/{}'",
            path,
            manifest::MANIFEST_FILENAME
        );
        if !self.ssh_command().arg(&check).status()?.success() {
            return Err(StorageBackendError::MissingSnapshot(format!(
                "no manifest found in snapshot '{snapshot_name}'; \
                 run a new backup to generate one"
            )));
        }

        let cmd = format!(
            "cd '{path}' && sha256sum --check '{mf}' 2>/dev/null",
            mf = manifest::MANIFEST_FILENAME,
        );
        let output = self.ssh_command().arg(&cmd).output()?;
        let stdout = String::from_utf8_lossy(&output.stdout);

        let mut ok_count = 0usize;
        let mut failures = Vec::new();

        for line in stdout.lines() {
            if line.ends_with(": OK") {
                ok_count += 1;
            } else if line.ends_with(": FAILED") {
                let p = line.trim_end_matches(": FAILED").to_string();
                failures.push(crate::core::manifest::VerifyFailure::ChecksumMismatch {
                    path: p,
                    expected: String::new(),
                    actual: String::new(),
                });
            } else if line.ends_with(": FAILED open or read") {
                let p = line.trim_end_matches(": FAILED open or read").to_string();
                failures.push(crate::core::manifest::VerifyFailure::Missing { path: p });
            }
        }

        Ok(VerifyReport { ok_count, failures })
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

    /// Removes the snapshot directory using [`std::fs::remove_dir_all`].
    fn delete_snapshot(&self, name: &str) -> Result<(), StorageBackendError> {
        validate_snapshot_name(name)?;
        let path = self.path.join(name);
        // Defense-in-depth: ensure the resolved path is still under self.path.
        // validate_snapshot_name already forbids "..", but this is an extra guard.
        if !path.starts_with(&self.path) {
            return Err(StorageBackendError::InvalidSnapshotName(name.to_string()));
        }
        std::fs::remove_dir_all(path)?;
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

    /// Generates a `SHA256SUMS` manifest inside the local snapshot directory.
    fn generate_manifest(&self, snapshot_name: &str) -> Result<(), StorageBackendError> {
        validate_snapshot_name(snapshot_name)?;
        let dir = self.path.join(snapshot_name);
        if !dir.starts_with(&self.path) {
            return Err(StorageBackendError::InvalidSnapshotName(snapshot_name.to_string()));
        }
        manifest::generate_local(&dir)?;
        Ok(())
    }

    /// Verifies the local snapshot by re-checksumming against its manifest.
    fn verify_snapshot(&self, snapshot_name: &str) -> Result<VerifyReport, StorageBackendError> {
        validate_snapshot_name(snapshot_name)?;
        let dir = self.path.join(snapshot_name);
        if !dir.starts_with(&self.path) {
            return Err(StorageBackendError::InvalidSnapshotName(snapshot_name.to_string()));
        }
        let manifest_path = dir.join(manifest::MANIFEST_FILENAME);
        if !manifest_path.exists() {
            return Err(StorageBackendError::MissingSnapshot(format!(
                "no manifest found in snapshot '{snapshot_name}'; \
                 run a new backup to generate one"
            )));
        }
        let report = manifest::verify_local(&dir)?;
        Ok(report)
    }
}
