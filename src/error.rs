//! Error type shared by every module.
//!
//! Each variant maps to a process exit code (see `docs/reference/cli.md#exit-codes`):
//! usage and configuration problems exit with 2, everything else with 1.

use std::path::PathBuf;

pub type Result<T, E = Error> = std::result::Result<T, E>;

#[derive(Debug, thiserror::Error)]
pub enum Error {
    /// Bad command-line usage that clap can't catch, e.g. conflicting options.
    #[error("{0}")]
    Usage(String),

    /// An invalid or unreadable `vers.toml`, a broken `extend` chain, or a
    /// `required_version` mismatch.
    #[error("{}: {message}", path.display())]
    Config { path: PathBuf, message: String },

    /// An invalid fragment: unknown type, unmatched reference, bad frontmatter,
    /// empty body.
    #[error("{}: {message}", path.display())]
    Fragment { path: PathBuf, message: String },

    /// A changelog that can't be parsed or updated.
    #[error("{}: {message}", path.display())]
    Changelog { path: PathBuf, message: String },

    /// A version file that can't be read or written, or version files that
    /// disagree.
    #[error("{}: {message}", path.display())]
    VersionFile { path: PathBuf, message: String },

    /// A failed git command.
    #[error("git: {0}")]
    Git(String),

    #[error("{}: {source}", path.display())]
    Io {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },

    /// The command ran but its outcome is a failure: `vers check` found
    /// problems, there is nothing to release, a version would go backwards.
    #[error("{0}")]
    Failed(String),
}

impl Error {
    pub fn exit_code(&self) -> i32 {
        match self {
            Error::Usage(_) | Error::Config { .. } => 2,
            _ => 1,
        }
    }

    pub fn config(path: impl Into<PathBuf>, message: impl Into<String>) -> Self {
        Error::Config { path: path.into(), message: message.into() }
    }

    pub fn fragment(path: impl Into<PathBuf>, message: impl Into<String>) -> Self {
        Error::Fragment { path: path.into(), message: message.into() }
    }

    pub fn changelog(path: impl Into<PathBuf>, message: impl Into<String>) -> Self {
        Error::Changelog { path: path.into(), message: message.into() }
    }

    pub fn version_file(path: impl Into<PathBuf>, message: impl Into<String>) -> Self {
        Error::VersionFile { path: path.into(), message: message.into() }
    }

    pub fn io(path: impl Into<PathBuf>, source: std::io::Error) -> Self {
        Error::Io { path: path.into(), source }
    }
}

/// Read a file, attaching its path to any error.
pub fn read_to_string(path: &std::path::Path) -> Result<String> {
    std::fs::read_to_string(path).map_err(|e| Error::io(path, e))
}
