//! `vers draft`: print the changelog section the next release would add.
//!
//! Spec: `docs/reference/cli.md#vers-draft`. [`build_release_for`] is the one
//! place a release section is built; `vers release` uses it too.

use serde::Serialize;

use crate::changelog::{self, Release};
use crate::cli::{DraftArgs, VersionChoiceArgs};
use crate::config::{self, Config, Selection};
use crate::context::{Context, release_date};
use crate::error::{Error, Result};
use crate::fragment::Fragment;
use crate::git::Git;
use crate::plan::{self, PackagePlan};
use crate::version::VersionSource;

pub fn run(ctx: &Context, args: &DraftArgs) -> Result<()> {
    let package = select_one(ctx, args.package.as_deref())?;
    let plan = plan_with_warnings(ctx, &package, &args.choice)?;
    let release = build_release_for(&plan)?.ok_or_else(|| nothing_to_release(package.name()))?;
    let markdown = changelog::render_release(&release);

    if ctx.json_output() {
        ctx.json(&DraftJson { package: package.name(), version: release.version.to_string(), markdown });
    } else {
        ctx.line(markdown.trim_end_matches('\n'));
    }
    Ok(())
}

/// The release section `plan` would add to the changelog, or `None` when there
/// is nothing to release.
///
/// Built from the visible pending fragments. When finalising a pre-release, or
/// releasing a final version with `[version] fold_prereleases`, the entries of
/// the version's pre-release sections come first.
pub fn build_release_for(plan: &PackagePlan) -> Result<Option<Release>> {
    let Some(next) = &plan.next else {
        return Ok(None);
    };
    let config = &plan.config;
    let fragments: Vec<Fragment> = plan.visible_fragments().cloned().collect();
    let mut release = changelog::build_release(next.next.clone(), release_date(), &fragments, &config.types, config);

    let finalising = next.source == VersionSource::FinalisePrerelease;
    let folding = next.next.pre.is_empty() && config.version.fold_prereleases;
    if (finalising || folding)
        && let Some(changelog) = &plan.changelog
    {
        release.merge_earlier(changelog.prerelease_sections(&next.next), &config.types);
    }

    if release.sections.is_empty() {
        return Ok(None);
    }
    Ok(Some(release))
}

/// The package named `name`, or the one owning the current directory.
pub(crate) fn select_one(ctx: &Context, name: Option<&str>) -> Result<Config> {
    let selection = match name {
        Some(name) => Selection::Named(vec![name.to_string()]),
        None => Selection::Current,
    };
    let mut packages = config::select_packages(&ctx.cwd, &selection, ctx.config_override.as_deref())?;
    // Exactly one: `Current` yields one config, `Named` errors on unknown names.
    Ok(packages.remove(0))
}

/// [`plan::plan_package`] for `config`, printing its warnings.
pub(crate) fn plan_with_warnings(ctx: &Context, config: &Config, choice: &VersionChoiceArgs) -> Result<PackagePlan> {
    let git = Git::discover(&ctx.cwd)?;
    let plan = plan::plan_package(config, git.as_ref(), choice, false)?;
    for warning in &plan.warnings {
        ctx.warn(warning);
    }
    Ok(plan)
}

pub(crate) fn nothing_to_release(name: &str) -> Error {
    Error::Failed(format!("nothing to release for `{name}`: no pending fragments"))
}

#[derive(Serialize)]
struct DraftJson<'a> {
    package: &'a str,
    version: String,
    markdown: String,
}
