use crate::core::location::LocationKind;
use std::path::PathBuf;
use thiserror::Error;

#[derive(Debug, Error)]
pub enum StorageBackendError {
    #[error("backup.marker not found at {0}; create it with: touch {0}/backup.marker")]
    MissingMarker(String),

    #[error("snapshot directory not found: {0}")]
    MissingSnapshot(String),

    #[error("io error: {0}")]
    Io(#[from] std::io::Error),

    #[error("ssh command failed: {0}")]
    Ssh(String),
}

pub trait StorageBackend {
    /// List all snapshot directories in the destination (format: YYYY-MM-DD-HHMMSS).
    fn list_snapshots(&self) -> Result<Vec<String>, StorageBackendError>;

    /// Check that backup.marker exists in the destination, error if not.
    //fn ensure_marker(&self) -> Result<(), StorageBackendError>;

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
}

pub struct LocalBackend {
    path: PathBuf,
}

pub struct RemoteBackend {
    user: String,
    host: String,
    path: String,
    ssh_port: u16,
    id_rsa: Option<PathBuf>,
}

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
    // fn ensure_marker(&self) -> Result<(), StorageBackendError> {
    //     let check = format!("test -f '{}/backup.marker'", self.path);
    //     let status = self.ssh_command().arg(&check).status()?;

    //     if !status.success() {
    //         return Err(StorageBackendError::MissingMarker(self.path.clone()));
    //     }
    //     Ok(())
    // }

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

        let snapshots = String::from_utf8_lossy(&output.stdout)
            .lines()
            .map(|l| l.to_string())
            .filter(|l| !l.is_empty())
            .collect();

        Ok(snapshots)
    }

    fn delete_snapshot(&self, name: &str) -> Result<(), StorageBackendError> {
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

    fn update_latest_symlink(&self, snapshot_name: &str) -> Result<(), StorageBackendError> {
        let cmd = format!(
            "ln -sfn '{}/{}' '{}/latest'",
            self.path, snapshot_name, self.path
        );
        let status = self.ssh_command().arg(&cmd).status()?;
        if !status.success() {
            return Err(StorageBackendError::Ssh(
                "failed to update latest symlink".to_string(),
            ));
        }
        Ok(())
    }

    fn rename_in_progress(&self, snapshot_name: &str) -> Result<(), StorageBackendError> {
        let cmd = format!("mv '{}/tmp' '{}/{}'", self.path, self.path, snapshot_name);
        let status = self.ssh_command().arg(&cmd).status()?;
        if !status.success() {
            return Err(StorageBackendError::Ssh(format!(
                "failed to rename tmp to {}",
                snapshot_name
            )));
        }
        Ok(())
    }

    fn latest_exists(&self) -> Result<bool, StorageBackendError> {
        let cmd = format!("test -e '{}/latest'", self.path);
        let status = self.ssh_command().arg(&cmd).status()?;
        Ok(status.success())
    }
}

impl StorageBackend for LocalBackend {
    // fn ensure_marker(&self) -> Result<(), StorageBackendError> {
    //     let marker = self.path.join("backup.marker");
    //     if !marker.exists() {
    //         return Err(StorageBackendError::MissingMarker(
    //             self.path.display().to_string(),
    //         ));
    //     }
    //     Ok(())
    // }

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

    fn delete_snapshot(&self, name: &str) -> Result<(), StorageBackendError> {
        let path = self.path.join(name);
        std::fs::remove_dir_all(path)?;
        Ok(())
    }

    fn update_latest_symlink(&self, snapshot_name: &str) -> Result<(), StorageBackendError> {
        let latest = self.path.join("latest");
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

    fn rename_in_progress(&self, snapshot_name: &str) -> Result<(), StorageBackendError> {
        let from = self.path.join("tmp");
        let to = self.path.join(snapshot_name);
        std::fs::rename(&from, &to)?;
        Ok(())
    }

    fn latest_exists(&self) -> Result<bool, StorageBackendError> {
        let latest = self.path.join("latest");
        Ok(latest.exists() || latest.is_symlink())
    }

}
