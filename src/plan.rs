//! The state of one package and what its next release would be. Shared by
//! `status`, `draft`, `version` and `release` so they always agree.
//!
//! Spec: `docs/concepts/versioning.md`.

use std::path::PathBuf;

use regex::Regex;
use semver::Version;

use crate::changelog::Changelog;
use crate::cli::VersionChoiceArgs;
use crate::config::Config;
use crate::error::Result;
use crate::fragment::{self, Fragment};
use crate::git::Git;
use crate::version::{self, ChooseInput, VersionPlan};

/// Everything known about a package before a release.
#[derive(Debug, Clone)]
pub struct PackagePlan {
    pub config: Config,
    /// The parsed changelog, or `None` if the file doesn't exist yet.
    pub changelog: Option<Changelog>,
    /// Pending fragments in release order, including hidden types.
    pub fragments: Vec<Fragment>,
    /// Latest released version: changelog, then matching git tags.
    pub current: Option<Version>,
    /// Agreed version of the version files and the first file's path.
    pub files_version: Option<(Version, PathBuf)>,
    /// The next release, or `None` when there is nothing to release.
    pub next: Option<VersionPlan>,
    /// Non-fatal problems to show the user (shallow clone, plain-text refs,
    /// version-file warnings from [`version::choose`]).
    pub warnings: Vec<String>,
}

impl PackagePlan {
    /// Fragments that will appear in the changelog (non-hidden types).
    pub fn visible_fragments(&self) -> impl Iterator<Item = &Fragment> {
        self.fragments.iter().filter(|f| self.config.find_type(&f.type_name).is_none_or(|t| !t.hidden))
    }
}

/// Load a package's state and choose its next version.
///
/// A package has something to release when it has at least one visible
/// fragment, or when its current version is a pre-release being finalised
/// (no `--pre`). Otherwise [`PackagePlan::next`] is `None`. Errors from
/// [`version::choose`] (e.g. a `--version` that isn't newer) are returned.
pub fn plan_package(
    config: &Config,
    git: Option<&Git>,
    choice: &VersionChoiceArgs,
    force: bool,
) -> Result<PackagePlan> {
    let mut warnings = Vec::new();

    let mut fragments = fragment::load_all(config)?;
    let added_times = match git {
        Some(git) if !fragments.is_empty() => {
            if git.is_shallow()? {
                warnings.push(
                    "this is a shallow clone, so fragment order may be wrong; \
                     fetch the full history (e.g. `fetch-depth: 0`)"
                        .to_string(),
                );
            }
            git.added_times(std::slice::from_ref(&config.fragments.directory))?
        }
        _ => Default::default(),
    };
    fragment::sort(&mut fragments, &added_times);

    for f in &fragments {
        for r in f.refs.iter().filter(|r| r.kind.is_none()) {
            warnings.push(format!(
                "{}: reference `{}` matches no reference kind and is shown as plain text",
                f.path.display(),
                r.raw
            ));
        }
    }

    let changelog = Changelog::load(&config.changelog.path)?;
    let current = match changelog.as_ref().and_then(Changelog::latest_version) {
        Some(v) => Some(v),
        None => match git {
            Some(git) => latest_tag_version(config, git)?,
            None => None,
        },
    };
    let files_version = version::agreed_version(&config.version.files, current.as_ref(), force)?;

    let has_visible = fragments.iter().any(|f| config.find_type(&f.type_name).is_none_or(|t| !t.hidden));
    let finalising = choice.pre.is_none() && current.as_ref().is_some_and(|v| !v.pre.is_empty());

    let next = if has_visible || finalising {
        let plan = version::choose(&ChooseInput {
            current: current.clone(),
            files_version: files_version.clone(),
            cli_version: choice.version.clone(),
            cli_bump: choice.bump,
            pre: choice.pre.clone(),
            fragments_bump: version::infer_bump(&fragments, &config.types),
            settings: &config.version,
        })?;
        warnings.extend(plan.warnings.iter().cloned());
        Some(plan)
    } else {
        None
    };

    Ok(PackagePlan { config: config.clone(), changelog, fragments, current, files_version, next, warnings })
}

/// Highest version among git tags matching the package's `tag_format`.
pub fn latest_tag_version(config: &Config, git: &Git) -> Result<Option<Version>> {
    let name = config.package.as_ref().map_or("", |p| p.name.as_str());
    let pattern = regex::escape(&config.version.tag_format)
        .replace(r"\{name\}", &regex::escape(name))
        .replace(r"\{version\}", r"(?<version>.+)");
    let re = Regex::new(&format!("^{pattern}$")).expect("escaped tag pattern is valid");
    Ok(git.tags()?.iter().filter_map(|t| re.captures(t)).filter_map(|c| Version::parse(&c["version"]).ok()).max())
}
