use std::path::PathBuf;
use std::str::FromStr;
use thiserror::Error;

/// Represents a parsed location, which can be either local or remote.
///
/// The original input string is preserved in [`Location::original`],
/// while the parsed representation is stored in [`Location::kind`].
#[derive(Debug, Clone)]
pub struct Location {
    pub original: String,
    pub kind: LocationKind,
}

/// Describes the type of a [`Location`].
#[derive(Debug, Clone)]
pub enum LocationKind {
    /// A local filesystem path.
    Local(PathBuf),
    /// A remote location in the form `user@host:/path`.
    Remote {
        /// The username used for the remote connection.
        user: String,
        /// The hostname or IP address of the remote machine.
        host: String,
        /// The path on the remote machine.
        path: String,
    },
}

impl Location {
    /// Returns `true` if this location is remote.
    ///
    /// # Examples
    ///
    /// ```
    /// use std::str::FromStr;
    ///
    /// let loc = "user@example.com:/tmp/file".parse::<Location>().unwrap();
    /// assert!(loc.is_remote());
    ///
    /// let loc = "/tmp/file".parse::<Location>().unwrap();
    /// assert!(!loc.is_remote());
    /// ```
    pub fn is_remote(&self) -> bool {
        matches!(self.kind, LocationKind::Remote { .. })
    }
}

/// Errors that can occur when parsing a [`Location`] from a string.
#[derive(Debug, Error)]
pub enum LocationParseError {
    /// The input string is not a valid remote path.
    ///
    /// Expected format: `user@host:/path`
    #[error("invalid remote path, expected user@host:/path or /path, got: {0}")]
    InvalidRemote(String),
}

impl FromStr for Location {
    type Err = LocationParseError;

    /// Parses a string into a [`Location`].
    ///
    /// # Supported formats
    ///
    /// - Local paths:
    ///   - `/some/local/path`
    ///   - `relative/path`
    ///
    /// - Remote paths:
    ///   - `user@host:/absolute/path`
    ///
    /// # Errors
    ///
    /// Returns [`LocationParseError::InvalidRemote`] if the string contains `:`
    /// but does not match the expected `user@host:/path` format.
    ///
    /// # Examples
    ///
    /// ```
    /// use std::str::FromStr;
    ///
    /// let local = Location::from_str("/tmp/file").unwrap();
    /// assert!(!local.is_remote());
    ///
    /// let remote = Location::from_str("user@host:/tmp/file").unwrap();
    /// assert!(remote.is_remote());
    /// ```
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let original = if s.ends_with('/') {
            s.to_string()
        } else {
            format!("{}/", s)
        };

        // if let Some((userhost, path)) = original.split_once(':') {
        //     if let Some((user, host)) = userhost.split_once('@') {
        //         return Ok(Location {
        //             original: original.clone(),
        //             kind: LocationKind::Remote {
        //                 user: user.to_string(),
        //                 host: host.to_string(),
        //                 path: path.to_string(),
        //             },
        //         });
        //     }
        //     return Err(LocationParseError::InvalidRemote(original));
        // }

        if let Some((userhost, path)) = s.split_once(':') {
            if let Some((user, host)) = userhost.split_once('@') {
                return Ok(Location {
                    original: original.clone(),
                    kind: LocationKind::Remote {
                        user: user.to_string(),
                        host: host.to_string(),
                        path: path.to_string(),
                    },
                });
            }
            // no @ means it's a host alias or bare hostname
            return Ok(Location {
                original: original.clone(),
                kind: LocationKind::Remote {
                    user: String::new(), // SSH config provides the user
                    host: userhost.to_string(),
                    path: path.to_string(),
                },
            });
        }

        Ok(Location {
            kind: LocationKind::Local(PathBuf::from(&original)),
            original,
        })
    }
}
