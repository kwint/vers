//! `vers status`: pending fragments and the next version of each package.
//!
//! Spec: `docs/reference/cli.md#vers-status`, `docs/reference/cli.md#json-output`.

use std::path::{Component, Path};

use serde::Serialize;

use crate::cli::{StatusArgs, VersionChoiceArgs};
use crate::config::{self, Selection};
use crate::context::{Context, display_relative};
use crate::error::Result;
use crate::git::Git;
use crate::plan::{self, PackagePlan};
use crate::version::{Bump, VersionPlan, VersionSource};

pub fn run(ctx: &Context, args: &StatusArgs) -> Result<()> {
    let selection = if args.packages.is_empty() { Selection::All } else { Selection::Named(args.packages.clone()) };
    let packages = config::select_packages(&ctx.cwd, &selection, ctx.config_override.as_deref())?;
    let git = Git::discover(&ctx.cwd)?;

    let mut plans = Vec::with_capacity(packages.len());
    for package in &packages {
        let plan = plan::plan_package(package, git.as_ref(), &VersionChoiceArgs::default(), false)?;
        for warning in &plan.warnings {
            ctx.warn(warning);
        }
        plans.push(plan);
    }

    if ctx.json_output() {
        let packages = plans.iter().map(|p| json_package(ctx, p)).collect();
        ctx.json(&StatusJson { packages });
        return Ok(());
    }

    let rows: Vec<[String; 3]> =
        plans.iter().map(|p| [p.config.name().to_string(), version_info(p), fragments_summary(p)]).collect();
    for line in format_columns(&rows) {
        ctx.line(line);
    }
    Ok(())
}

/// How the release version was chosen, as shown in parentheses by `status` and
/// `release`: `inferred: minor`, `from VERSION`, `from command line`, `initial`
/// or `finalising pre-release`. Version-file paths are shown relative to the
/// package directory (`package_dir`).
pub fn source_label(next: &VersionPlan, package_dir: &Path) -> String {
    match (&next.source, &next.current) {
        (VersionSource::Inferred(_), None) => "initial".to_string(),
        (VersionSource::Inferred(bump), Some(_)) => format!("inferred: {bump}"),
        (VersionSource::Files(path), _) => format!("from {}", display_relative(path, package_dir)),
        (VersionSource::Cli, _) => "from command line".to_string(),
        (VersionSource::FinalisePrerelease, _) => "finalising pre-release".to_string(),
    }
}

/// The `version_source` value in JSON output.
fn source_kind(next: &VersionPlan) -> &'static str {
    match (&next.source, &next.current) {
        (VersionSource::Inferred(_), None) => "initial",
        (VersionSource::Inferred(_), Some(_)) => "inferred",
        (VersionSource::Files(_), _) => "files",
        (VersionSource::Cli, _) => "cli",
        (VersionSource::FinalisePrerelease, _) => "finalise",
    }
}

/// `<current> → <next> (<source>)`, or just `<current>` with nothing to release.
fn version_info(plan: &PackagePlan) -> String {
    let current = plan.current.as_ref().map_or_else(|| "(unreleased)".to_string(), ToString::to_string);
    match &plan.next {
        Some(next) => format!("{current} → {} ({})", next.next, source_label(next, &plan.config.dir)),
        None => current,
    }
}

/// `2 fragments: 1 added, 1 fixed` (per type, in configured type order), or
/// `no fragments`.
fn fragments_summary(plan: &PackagePlan) -> String {
    let total = plan.fragments.len();
    if total == 0 {
        return "no fragments".to_string();
    }
    let mut counts: Vec<String> = plan
        .config
        .types
        .iter()
        .filter_map(|t| {
            let n = plan.fragments.iter().filter(|f| f.type_name.eq_ignore_ascii_case(&t.name)).count();
            (n > 0).then(|| format!("{n} {}", t.name))
        })
        .collect();
    // Fragment types are always canonical config types, but don't lose any
    // that somehow aren't.
    let counted: usize = plan
        .fragments
        .iter()
        .filter(|f| plan.config.types.iter().any(|t| t.name.eq_ignore_ascii_case(&f.type_name)))
        .count();
    if counted < total {
        counts.push(format!("{} other", total - counted));
    }
    let noun = if total == 1 { "fragment" } else { "fragments" };
    format!("{total} {noun}: {}", counts.join(", "))
}

/// Join columns with two spaces, padding every column but the last to its
/// widest value.
fn format_columns(rows: &[[String; 3]]) -> Vec<String> {
    let width = |col: usize| rows.iter().map(|r| r[col].chars().count()).max().unwrap_or(0);
    let widths = [width(0), width(1)];
    rows.iter()
        .map(|r| {
            let pad = |col: usize| {
                let value = &r[col];
                format!("{value}{}", " ".repeat(widths[col] - value.chars().count()))
            };
            format!("{}  {}  {}", pad(0), pad(1), r[2])
        })
        .collect()
}

/// `path` relative to `base` with `/` separators, using `..` when `path` isn't
/// below `base`; `.` when they're equal. Falls back to the full path when
/// they share no root (e.g. different drives on Windows).
pub(crate) fn relative_path(path: &Path, base: &Path) -> String {
    let path_parts: Vec<Component> = path.components().collect();
    let base_parts: Vec<Component> = base.components().collect();
    let common = path_parts.iter().zip(&base_parts).take_while(|(a, b)| a == b).count();
    if common == 0 {
        return path.to_string_lossy().replace('\\', "/");
    }
    let mut parts: Vec<String> = Vec::new();
    for _ in common..base_parts.len() {
        parts.push("..".to_string());
    }
    for c in &path_parts[common..] {
        parts.push(c.as_os_str().to_string_lossy().into_owned());
    }
    if parts.is_empty() { ".".to_string() } else { parts.join("/") }
}

#[derive(Serialize)]
struct StatusJson {
    packages: Vec<PackageJson>,
}

#[derive(Serialize)]
struct PackageJson {
    name: String,
    path: String,
    current_version: Option<String>,
    next_version: Option<String>,
    version_source: Option<&'static str>,
    bump: Option<Bump>,
    fragments: Vec<FragmentJson>,
}

#[derive(Serialize)]
struct FragmentJson {
    path: String,
    #[serde(rename = "type")]
    type_name: String,
    refs: Vec<String>,
    breaking: bool,
}

fn json_package(ctx: &Context, plan: &PackagePlan) -> PackageJson {
    let next = plan.next.as_ref();
    PackageJson {
        name: plan.config.name().to_string(),
        path: relative_path(&plan.config.dir, &ctx.cwd),
        current_version: plan.current.as_ref().map(ToString::to_string),
        next_version: next.map(|n| n.next.to_string()),
        version_source: next.map(source_kind),
        bump: next.and_then(|n| match (&n.source, &n.current) {
            (VersionSource::Inferred(bump), Some(_)) => Some(*bump),
            _ => None,
        }),
        fragments: plan
            .fragments
            .iter()
            .map(|f| FragmentJson {
                path: relative_path(&f.path, &ctx.cwd),
                type_name: f.type_name.clone(),
                refs: f.refs.iter().map(|r| r.raw.clone()).collect(),
                breaking: f.breaking,
            })
            .collect(),
    }
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use super::*;

    #[test]
    fn relative_paths() {
        let base = PathBuf::from("/a/b");
        assert_eq!(relative_path(Path::new("/a/b"), &base), ".");
        assert_eq!(relative_path(Path::new("/a/b/c/d.md"), &base), "c/d.md");
        assert_eq!(relative_path(Path::new("/a"), &base), "..");
        assert_eq!(relative_path(Path::new("/a/x/y"), &base), "../x/y");
    }

    #[test]
    fn columns_are_padded_by_characters() {
        let rows = [
            ["api".to_string(), "1.4.0 → 1.5.0 (inferred: minor)".to_string(), "x".to_string()],
            ["web-app".to_string(), "0.3.0".to_string(), "no fragments".to_string()],
        ];
        assert_eq!(
            format_columns(&rows),
            ["api      1.4.0 → 1.5.0 (inferred: minor)  x", "web-app  0.3.0                            no fragments"]
        );
    }
}
