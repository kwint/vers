//! Versions: bump inference, choosing the release version, and reading and
//! writing version files.
//!
//! Spec: `docs/concepts/versioning.md`, `docs/how-to/manage-version-files.md`,
//! `docs/how-to/control-the-release-version.md`.

use std::cmp::Ordering;
use std::path::PathBuf;

use semver::{BuildMetadata, Prerelease, Version};
use serde::Serialize;

use crate::config::{ChangeType, VersionFileSpec, VersionLocator, VersionSettings};
use crate::error::{Error, Result};
use crate::fragment::Fragment;

mod locate;

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
    fragments
        .iter()
        .filter_map(|f| {
            let ty = types.iter().find(|t| t.name.eq_ignore_ascii_case(&f.type_name))?;
            Some(if f.breaking { Bump::Major } else { ty.bump })
        })
        .max()
        .unwrap_or(Bump::None)
}

/// Apply `bump` to `current`, honouring `zero_major_shift` on 0.x versions.
/// Applying a bump to a pre-release whose base already reflects that bump
/// returns the base (e.g. `1.3.0-rc.2` + minor = `1.3.0`).
pub fn apply_bump(current: &Version, bump: Bump, zero_major_shift: bool) -> Version {
    if bump == Bump::None {
        return current.clone();
    }
    let bump = if zero_major_shift && current.major == 0 {
        match bump {
            Bump::Major => Bump::Minor,
            _ => Bump::Patch,
        }
    } else {
        bump
    };
    let (x, y, z) = (current.major, current.minor, current.patch);
    let is_pre = !current.pre.is_empty();
    match bump {
        Bump::Major if is_pre && y == 0 && z == 0 => Version::new(x, 0, 0),
        Bump::Major => Version::new(x + 1, 0, 0),
        Bump::Minor if is_pre && z == 0 => Version::new(x, y, 0),
        Bump::Minor => Version::new(x, y + 1, 0),
        Bump::Patch if is_pre => Version::new(x, y, z),
        Bump::Patch => Version::new(x, y, z + 1),
        Bump::None => unreachable!("handled above"),
    }
}

/// Whether `a` is newer than `b` by SemVer precedence (build metadata ignored).
fn newer(a: &Version, b: &Version) -> bool {
    a.cmp_precedence(b) == Ordering::Greater
}

/// `v` without pre-release and build metadata.
fn base_of(v: &Version) -> Version {
    Version::new(v.major, v.minor, v.patch)
}

/// The number of a pre-release `<id>.<n>`, if `pre` has that shape.
fn pre_number(pre: &Prerelease, id: &str) -> Option<u64> {
    pre.as_str().strip_prefix(id)?.strip_prefix('.')?.parse().ok()
}

/// Turn `target` into a pre-release with identifier `pre`: the next number
/// after `current` when that is a pre-release of the same base with the same
/// identifier, otherwise `.1`. Without `pre`, returns `target` unchanged.
fn with_pre(target: Version, current: Option<&Version>, pre: Option<&str>) -> Result<Version> {
    let Some(id) = pre else {
        return Ok(target);
    };
    let mut next = base_of(&target);
    let number = match current {
        Some(c) if !c.pre.is_empty() && base_of(c) == next => pre_number(&c.pre, id).map_or(1, |n| n + 1),
        _ => 1,
    };
    next.pre = Prerelease::new(&format!("{id}.{number}"))
        .map_err(|e| Error::Usage(format!("invalid pre-release identifier `{id}`: {e}")))?;
    next.build = BuildMetadata::EMPTY;
    Ok(next)
}

fn validate_pre(pre: &str) -> Result<()> {
    let valid = !pre.is_empty() && pre.chars().all(|c| c.is_ascii_alphanumeric() || c == '-');
    if valid {
        Ok(())
    } else {
        Err(Error::Usage(format!(
            "invalid pre-release identifier `{pre}`: use letters, digits and hyphens only, such as `rc` or `beta`"
        )))
    }
}

/// Choose the release version: CLI, then version files if newer, then
/// inference. See `docs/concepts/versioning.md#the-release-version`.
/// Errors (as [`crate::Error::Failed`]) if the result isn't newer than `current`.
pub fn choose(input: &ChooseInput<'_>) -> Result<VersionPlan> {
    let settings = input.settings;
    let shift = settings.zero_major_shift;
    let current = input.current.as_ref();
    let pre = input.pre.as_deref();
    if let Some(p) = pre {
        validate_pre(p)?;
    }
    let mut warnings = Vec::new();

    let files_newer = input.files_version.as_ref().filter(|(v, _)| current.is_none_or(|c| newer(v, c)));

    let (next, source) = if let Some(v) = &input.cli_version {
        (v.clone(), VersionSource::Cli)
    } else if let Some(bump) = input.cli_bump {
        let target = match current {
            Some(c) => apply_bump(c, bump, shift),
            None => settings.initial.clone(),
        };
        (with_pre(target, current, pre)?, VersionSource::Cli)
    } else if let Some((files, path)) = files_newer {
        if let Some(c) = current {
            let suggested = apply_bump(c, input.fragments_bump, shift);
            if newer(&suggested, files) {
                warnings.push(format!(
                    "{} sets version {files}, but the pending fragments suggest a {} bump ({suggested}); \
                     releasing {files}",
                    path.display(),
                    input.fragments_bump,
                ));
            }
        }
        let next = if files.pre.is_empty() { with_pre(files.clone(), current, pre)? } else { files.clone() };
        (next, VersionSource::Files(path.clone()))
    } else {
        match current {
            None => (with_pre(settings.initial.clone(), None, pre)?, VersionSource::Inferred(input.fragments_bump)),
            Some(c) if !c.pre.is_empty() && pre.is_none() && input.fragments_bump == Bump::None => {
                (base_of(c), VersionSource::FinalisePrerelease)
            }
            Some(c) => {
                let target = apply_bump(c, input.fragments_bump, shift);
                (with_pre(target, current, pre)?, VersionSource::Inferred(input.fragments_bump))
            }
        }
    };

    if let Some(c) = current
        && !newer(&next, c)
    {
        return Err(Error::Failed(format!(
            "version {next} is not newer than the current version {c}; vers never releases the same version \
             twice or goes backwards"
        )));
    }

    Ok(VersionPlan { current: input.current.clone(), next, source, warnings })
}

/// Read the version from one version file.
pub fn read_file_version(spec: &VersionFileSpec) -> Result<Version> {
    let content = crate::error::read_to_string(&spec.path)?;
    let span = locate::locate(spec, &content)?;
    let raw = &content[span];
    Version::parse(raw).map_err(|e| {
        let at = match &spec.locator {
            VersionLocator::Key(key) => format!(" at key `{key}`"),
            _ => String::new(),
        };
        let hint = if raw.starts_with(['v', 'V']) && Version::parse(&raw[1..]).is_ok() {
            "; hint: remove the leading `v`"
        } else {
            ""
        };
        Error::version_file(&spec.path, format!("invalid version `{raw}`{at}: {e}{hint}"))
    })
}

/// Read every version file and check consistency: all must agree, and none may
/// be older than `current`. With `force`, both checks are skipped and the
/// highest version wins. Returns `None` when there are no version files.
pub fn agreed_version(
    specs: &[VersionFileSpec],
    current: Option<&Version>,
    force: bool,
) -> Result<Option<(Version, PathBuf)>> {
    let versions =
        specs.iter().map(|s| read_file_version(s).map(|v| (v, s.path.clone()))).collect::<Result<Vec<_>>>()?;
    let Some((first, first_path)) = versions.first().cloned() else {
        return Ok(None);
    };

    if force {
        // Highest by precedence; the first file holding it wins ties.
        let best = versions
            .into_iter()
            .reduce(|best, cand| if newer(&cand.0, &best.0) { cand } else { best })
            .expect("not empty");
        return Ok(Some(best));
    }

    if versions.iter().any(|(v, _)| *v != first) {
        let list = versions.iter().map(|(v, p)| format!("\n  {}: {v}", p.display())).collect::<String>();
        return Err(Error::version_file(
            &first_path,
            format!(
                "version files disagree:{list}\nhint: set the same version in every file, or pass --force to use \
                 the highest"
            ),
        ));
    }

    if let Some(c) = current
        && newer(c, &first)
    {
        return Err(Error::version_file(
            &first_path,
            format!(
                "version {first} is older than the current version {c}; a release was probably made without vers \
                 (hint: set the file to {c} or newer, or pass --force)"
            ),
        ));
    }

    Ok(Some((first, first_path)))
}

/// Compute the new contents of a version file with its version replaced by
/// `new`, keeping all other formatting. Does not write the file.
pub fn update_file_version(spec: &VersionFileSpec, new: &Version) -> Result<String> {
    let content = crate::error::read_to_string(&spec.path)?;
    let span = locate::locate(spec, &content)?;
    let mut out = String::with_capacity(content.len() + 8);
    out.push_str(&content[..span.start]);
    out.push_str(&new.to_string());
    out.push_str(&content[span.end..]);
    Ok(out)
}

#[cfg(test)]
mod tests;
