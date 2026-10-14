//! Versions: bump inference, choosing the release version, and reading and
//! writing version files.
//!
//! Spec: `docs/concepts/versioning.md`, `docs/how-to/manage-version-files.md`,
//! `docs/how-to/control-the-release-version.md`.

use std::path::PathBuf;

use semver::Version;
use serde::Serialize;

use crate::config::{ChangeType, VersionFileSpec, VersionSettings};
use crate::error::Result;
use crate::fragment::Fragment;

/// A SemVer bump level. Ordered: `None < Patch < Minor < Major`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Bump {
    None,
    Patch,
    Minor,
    Major,
}

impl std::str::FromStr for Bump {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "none" => Ok(Bump::None),
            "patch" => Ok(Bump::Patch),
            "minor" => Ok(Bump::Minor),
            "major" => Ok(Bump::Major),
            other => Err(format!("unknown bump level `{other}` (expected major, minor, patch or none)")),
        }
    }
}

impl std::fmt::Display for Bump {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            Bump::None => "none",
            Bump::Patch => "patch",
            Bump::Minor => "minor",
            Bump::Major => "major",
        })
    }
}

/// Where the release version came from. Shown by `vers status` and `release`
/// as e.g. `(inferred: minor)` or `(from VERSION)`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum VersionSource {
    /// `--version` or `--bump`.
    Cli,
    /// The version files were newer than the current version. Holds the path
    /// of the first version file, for display.
    Files(PathBuf),
    /// Inferred from the fragments.
    Inferred(Bump),
    /// Finalising a pre-release (`1.3.0-rc.2` → `1.3.0`).
    FinalisePrerelease,
}

/// Inputs for [`choose`].
#[derive(Debug, Clone)]
pub struct ChooseInput<'a> {
    /// Latest released version (changelog, then tags), if any.
    pub current: Option<Version>,
    /// The agreed version of the version files, if there are any (see [`agreed_version`]).
    pub files_version: Option<(Version, PathBuf)>,
    pub cli_version: Option<Version>,
    pub cli_bump: Option<Bump>,
    /// Pre-release identifier from `--pre`, e.g. `rc`.
    pub pre: Option<String>,
    /// Highest bump implied by the pending fragments (see [`infer_bump`]).
    pub fragments_bump: Bump,
    pub settings: &'a VersionSettings,
}

/// The chosen release version.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VersionPlan {
    pub current: Option<Version>,
    pub next: Version,
    pub source: VersionSource,
    /// Non-fatal problems, e.g. version files giving a smaller bump than the
    /// fragments imply.
    pub warnings: Vec<String>,
}

/// The highest bump implied by `fragments`: each type's `bump`, or `Major`
/// for breaking fragments. Fragments of unknown types are ignored.
pub fn infer_bump(fragments: &[Fragment], types: &[ChangeType]) -> Bump {
    let _ = (fragments, types);
    todo!("phase 1: version")
}

/// Apply `bump` to `current`, honouring `zero_major_shift` on 0.x versions.
/// Applying a bump to a pre-release whose base already reflects that bump
/// returns the base (e.g. `1.3.0-rc.2` + minor = `1.3.0`).
pub fn apply_bump(current: &Version, bump: Bump, zero_major_shift: bool) -> Version {
    let _ = (current, bump, zero_major_shift);
    todo!("phase 1: version")
}

/// Choose the release version: CLI, then version files if newer, then
/// inference. See `docs/concepts/versioning.md#the-release-version`.
/// Errors (as [`crate::Error::Failed`]) if the result isn't newer than `current`.
pub fn choose(input: &ChooseInput<'_>) -> Result<VersionPlan> {
    let _ = input;
    todo!("phase 1: version")
}

/// Read the version from one version file.
pub fn read_file_version(spec: &VersionFileSpec) -> Result<Version> {
    let _ = spec;
    todo!("phase 1: version")
}

/// Read every version file and check consistency: all must agree, and none may
/// be older than `current`. With `force`, both checks are skipped and the
/// highest version wins. Returns `None` when there are no version files.
pub fn agreed_version(
    specs: &[VersionFileSpec],
    current: Option<&Version>,
    force: bool,
) -> Result<Option<(Version, PathBuf)>> {
    let _ = (specs, current, force);
    todo!("phase 1: version")
}

/// Compute the new contents of a version file with its version replaced by
/// `new`, keeping all other formatting. Does not write the file.
pub fn update_file_version(spec: &VersionFileSpec, new: &Version) -> Result<String> {
    let _ = (spec, new);
    todo!("phase 1: version")
}
