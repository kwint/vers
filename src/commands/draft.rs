//! `vers draft`: print the changelog section the next release would add.
//!
//! Spec: `docs/reference/cli.md#vers-draft`. [`build_release_for`] is the one
//! place a release section is built; `vers release` uses it too.

use std::path::{Path, PathBuf};

use serde::Serialize;

use crate::changelog::{self, Release};
use crate::cli::{DraftArgs, VersionChoiceArgs};
use crate::commands::status::relative_path;
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

    // Finalising a pre-release without new visible fragments: the content
    // comes from the pre-release sections, with or without folding. This also
    // covers pending fragments that are all hidden, which `version::choose`
    // reports as inferred rather than as finalising.
    let finalising = next.source == VersionSource::FinalisePrerelease || fragments.is_empty();
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
    plan_and_warn(ctx, config, git.as_ref(), choice)
}

/// [`plan::plan_package`] for `config`, printing its warnings. Paths in the
/// warnings and in a returned error are shown relative to the current
/// directory.
pub(crate) fn plan_and_warn(
    ctx: &Context,
    config: &Config,
    git: Option<&Git>,
    choice: &VersionChoiceArgs,
) -> Result<PackagePlan> {
    let plan = plan::plan_package(config, git, choice, false).map_err(|e| relative_error(ctx, config, e))?;
    let fragment_paths: Vec<PathBuf> = plan.fragments.iter().map(|f| f.path.clone()).collect();
    for warning in &plan.warnings {
        ctx.warn(relativise(ctx, config, &fragment_paths, warning));
    }
    Ok(plan)
}

/// `text` with the absolute paths of `config`'s files (`extra`, the version
/// files, the changelog and the config file) shown relative to the current
/// directory. Core modules format these paths with `Path::display`, so
/// they'd otherwise be absolute.
fn relativise(ctx: &Context, config: &Config, extra: &[PathBuf], text: &str) -> String {
    let mut paths: Vec<&Path> = extra.iter().map(PathBuf::as_path).collect();
    paths.extend(config.version.files.iter().map(|f| f.path.as_path()));
    paths.extend([config.changelog.path.as_path(), config.path.as_path()]);
    // Longest first, so a path never replaces part of a longer one.
    paths.sort_by_key(|p| std::cmp::Reverse(p.as_os_str().len()));
    let mut text = text.to_string();
    for path in paths {
        let shown = path.display().to_string();
        if !shown.is_empty() && text.contains(&shown) {
            text = text.replace(&shown, &relative_path(path, &ctx.cwd));
        }
    }
    text
}

/// `error` with its path and message shown relative to the current directory
/// (see [`relativise`]).
fn relative_error(ctx: &Context, config: &Config, error: Error) -> Error {
    let path = |p: PathBuf| PathBuf::from(relative_path(&p, &ctx.cwd));
    let message = |m: String| relativise(ctx, config, &[], &m);
    match error {
        Error::Fragment { path: p, message: m } => Error::Fragment { path: path(p), message: message(m) },
        Error::Changelog { path: p, message: m } => Error::Changelog { path: path(p), message: message(m) },
        Error::VersionFile { path: p, message: m } => Error::VersionFile { path: path(p), message: message(m) },
        Error::Io { path: p, source } => Error::Io { path: path(p), source },
        Error::Failed(m) => Error::Failed(message(m)),
        other => other,
    }
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
