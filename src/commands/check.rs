//! `vers check`: fail when a branch changes a package without adding a
//! fragment, and validate every fragment the branch adds or modifies.
//!
//! Spec: `docs/reference/cli.md#vers-check`,
//! `docs/how-to/require-fragments-in-ci.md`, `docs/concepts/monorepos.md`.

use std::collections::{BTreeMap, HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::rc::Rc;

use globset::{Glob, GlobSet, GlobSetBuilder};
use serde::Serialize;

use crate::cli::CheckArgs;
use crate::config::{self, Config};
use crate::context::Context;
use crate::error::{Error, Result};
use crate::fragment;
use crate::git::{self, Git};

/// How many changed files a failure message lists before summarising.
const MAX_LISTED: usize = 5;

pub fn run(ctx: &Context, args: &CheckArgs) -> Result<()> {
    if std::env::var("VERS_SKIP").is_ok_and(|v| v.trim() == "1") {
        if ctx.json_output() {
            ctx.json(&Report {
                ok: true,
                skipped: true,
                base: None,
                packages: Vec::new(),
                invalid_fragments: Vec::new(),
            });
        } else {
            ctx.line("Skipped (VERS_SKIP=1)");
        }
        return Ok(());
    }

    let git = Git::discover(&ctx.cwd)?.ok_or_else(|| {
        Error::Usage(
            "`vers check` compares the current branch with a base branch, so it must run inside a git repository"
                .to_string(),
        )
    })?;
    let base = args.base.clone().unwrap_or_else(git::default_check_base);
    if git.is_shallow()? {
        ctx.warn(
            "this is a shallow clone, so the changes since the base branch may be incomplete; \
             fetch the full history (`fetch-depth: 0` in GitHub Actions, `GIT_DEPTH: 0` in GitLab CI)",
        );
    }

    let changed = git.changed_files(&base)?;
    let added_or_modified: HashSet<PathBuf> = git.added_or_modified_files(&base)?.into_iter().collect();

    let mut resolver = Resolver::new(ctx)?;
    let mut packages: BTreeMap<PathBuf, PackageState> = BTreeMap::new();
    let mut invalid: Vec<(PathBuf, String)> = Vec::new();

    for file in &changed {
        let Some(config) = resolver.config_for(file)? else { continue };
        if !in_scope(&ctx.cwd, &config.dir) {
            continue;
        }
        let in_fragment_dir = file.parent() == Some(config.fragments.directory.as_path());
        let is_new_fragment = in_fragment_dir && is_fragment_file(file) && added_or_modified.contains(file);

        if is_new_fragment && file.is_file() {
            if !config.is_package() {
                invalid.push((
                    file.clone(),
                    format!(
                        "this fragment is under {}, which has no [package] table, so no package can release it; \
                         move it to a package's fragment directory",
                        ctx.display_path(&config.path)
                    ),
                ));
                continue;
            }
            if let Err(e) = fragment::load(file, &config) {
                invalid.push((file.clone(), error_message(e)));
            }
        }
        if !config.is_package() {
            continue;
        }

        let state = packages.entry(config.path.clone()).or_insert_with(|| PackageState::new(Rc::clone(&config)));
        if in_fragment_dir {
            if is_new_fragment {
                state.fragments.push(file.clone());
            }
        } else if *file == config.changelog.path {
            state.changelog_updated = true;
            state.changed.push(file.clone());
        } else if state.ignore.is_match(relative_slash(file, &config.dir)) {
            state.ignored.push(file.clone());
        } else {
            state.changed.push(file.clone());
        }
    }

    let mut problems: Vec<String> = Vec::new();
    let mut reports: Vec<PackageReport> = Vec::new();
    let mut any_changes = false;
    for state in packages.values() {
        let config = &state.config;
        any_changes |= !state.changed.is_empty();
        let reason = if !state.fragments.is_empty() {
            "fragment_added"
        } else if state.changed.is_empty() {
            "only_ignored_files"
        } else if !config.check.require_fragment {
            "fragment_not_required"
        } else if state.changelog_updated {
            "changelog_updated"
        } else {
            problems.push(missing_fragment_message(ctx, config.name(), &state.changed));
            "missing_fragment"
        };
        reports.push(PackageReport {
            name: config.name().to_string(),
            path: dir_display(ctx, &config.dir),
            changed: state.changed.iter().map(|p| ctx.display_path(p)).collect(),
            ignored: state.ignored.iter().map(|p| ctx.display_path(p)).collect(),
            fragments: state.fragments.iter().map(|p| ctx.display_path(p)).collect(),
            ok: reason != "missing_fragment",
            reason,
        });
    }
    for (path, message) in &invalid {
        problems.push(format!("invalid fragment {}: {message}", ctx.display_path(path)));
    }

    let ok = problems.is_empty();
    if ctx.json_output() {
        ctx.json(&Report {
            ok,
            skipped: false,
            base: Some(base),
            packages: reports,
            invalid_fragments: invalid
                .iter()
                .map(|(path, error)| InvalidFragment { path: ctx.display_path(path), error: error.clone() })
                .collect(),
        });
    }
    if !ok {
        return Err(Error::Failed(problems.join("\n")));
    }
    if any_changes {
        ctx.line("All changed packages have fragments");
    } else {
        ctx.line("No package changes");
    }
    Ok(())
}

#[derive(Serialize)]
struct Report {
    ok: bool,
    skipped: bool,
    base: Option<String>,
    packages: Vec<PackageReport>,
    invalid_fragments: Vec<InvalidFragment>,
}

#[derive(Serialize)]
struct PackageReport {
    name: String,
    path: String,
    /// Files that need a fragment (not ignored, not fragments).
    changed: Vec<String>,
    /// Files matching `[check] ignore`.
    ignored: Vec<String>,
    /// New or modified fragments.
    fragments: Vec<String>,
    ok: bool,
    /// Why the package passed or failed: `fragment_added`,
    /// `only_ignored_files`, `fragment_not_required`, `changelog_updated` or
    /// `missing_fragment`.
    reason: &'static str,
}

#[derive(Serialize)]
struct InvalidFragment {
    path: String,
    error: String,
}

struct PackageState {
    config: Rc<Config>,
    ignore: GlobSet,
    changed: Vec<PathBuf>,
    ignored: Vec<PathBuf>,
    fragments: Vec<PathBuf>,
    changelog_updated: bool,
}

impl PackageState {
    fn new(config: Rc<Config>) -> PackageState {
        let ignore = build_globset(&config.check.ignore);
        PackageState {
            config,
            ignore,
            changed: Vec::new(),
            ignored: Vec::new(),
            fragments: Vec::new(),
            changelog_updated: false,
        }
    }
}

/// `check.ignore` as a glob set. The globs were validated when the config was
/// loaded; any that still fail to compile are skipped.
fn build_globset(globs: &[String]) -> GlobSet {
    let mut builder = GlobSetBuilder::new();
    for glob in globs {
        if let Ok(glob) = Glob::new(glob) {
            builder.add(glob);
        }
    }
    builder.build().unwrap_or_else(|_| GlobSet::empty())
}

/// Finds the config for each changed file, loading every config only once.
struct Resolver {
    /// `--config`: the config for every file.
    fixed: Option<Rc<Config>>,
    /// Directory → config file that applies to it.
    dirs: HashMap<PathBuf, Option<PathBuf>>,
    /// Config file → loaded config.
    configs: HashMap<PathBuf, Rc<Config>>,
}

impl Resolver {
    fn new(ctx: &Context) -> Result<Resolver> {
        let fixed = match &ctx.config_override {
            Some(path) => Some(Rc::new(config::load(path)?)),
            None => None,
        };
        Ok(Resolver { fixed, dirs: HashMap::new(), configs: HashMap::new() })
    }

    fn config_for(&mut self, file: &Path) -> Result<Option<Rc<Config>>> {
        if let Some(config) = &self.fixed {
            return Ok(Some(Rc::clone(config)));
        }
        let Some(dir) = file.parent() else { return Ok(None) };
        let config_path = match self.dirs.get(dir) {
            Some(found) => found.clone(),
            None => {
                // `find_config_for` starts at the parent of a file and walks
                // up, so deleted files and directories work too.
                let found = config::find_config_for(file)?;
                self.dirs.insert(dir.to_path_buf(), found.clone());
                found
            }
        };
        let Some(config_path) = config_path else { return Ok(None) };
        if let Some(config) = self.configs.get(&config_path) {
            return Ok(Some(Rc::clone(config)));
        }
        let config = Rc::new(config::load(&config_path)?);
        self.configs.insert(config_path, Rc::clone(&config));
        Ok(Some(config))
    }
}

/// Packages at or below the current directory, and the one containing it.
fn in_scope(cwd: &Path, dir: &Path) -> bool {
    dir.starts_with(cwd) || cwd.starts_with(dir)
}

/// Whether a file in a fragment directory is a fragment (as `load_all` sees it).
fn is_fragment_file(path: &Path) -> bool {
    let name = path.file_name().map(|n| n.to_string_lossy()).unwrap_or_default();
    name.ends_with(".md") && !name.eq_ignore_ascii_case("README.md")
}

/// `path` relative to `dir` with `/` separators, for matching ignore globs.
fn relative_slash(path: &Path, dir: &Path) -> String {
    let rel = path.strip_prefix(dir).unwrap_or(path);
    rel.components().map(|c| c.as_os_str().to_string_lossy().into_owned()).collect::<Vec<_>>().join("/")
}

fn dir_display(ctx: &Context, dir: &Path) -> String {
    let shown = ctx.display_path(dir);
    if shown.is_empty() { ".".to_string() } else { shown }
}

/// The message of a fragment error, without the path (which the caller shows
/// relative to the current directory).
fn error_message(e: Error) -> String {
    match e {
        Error::Fragment { message, .. } => message,
        other => other.to_string(),
    }
}

fn missing_fragment_message(ctx: &Context, name: &str, changed: &[PathBuf]) -> String {
    let mut message = format!("package `{name}` has changes but no new fragment");
    for path in changed.iter().take(MAX_LISTED) {
        message.push_str(&format!("\n  changed: {}", ctx.display_path(path)));
    }
    if changed.len() > MAX_LISTED {
        message.push_str(&format!("\n  … and {} more", changed.len() - MAX_LISTED));
    }
    message.push_str(&format!(
        "\n  hint: add one with `vers new <type> --package {name}`, or set VERS_SKIP=1 to skip this check"
    ));
    message
}
