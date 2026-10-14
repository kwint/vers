//! `vers migrate towncrier` and `vers init --from-towncrier`.
//!
//! Spec: `docs/how-to/migrate-from-towncrier.md`, `docs/reference/cli.md`.

mod headings;
mod init;
mod towncrier;

use std::collections::HashSet;
use std::ffi::OsString;
use std::path::{Path, PathBuf};

use serde::Serialize;

use crate::changelog::Changelog;
use crate::cli::{InitArgs, MigrateTowncrierArgs};
use crate::config::{self, Config};
use crate::context::Context;
use crate::error::{Error, Result};
use crate::fragment;
use crate::git::Git;

use towncrier::{Target, TowncrierConfig};

// ---------------------------------------------------------------------------
// vers migrate towncrier
// ---------------------------------------------------------------------------

pub fn run(ctx: &Context, args: &MigrateTowncrierArgs) -> Result<()> {
    let overrides = parse_overrides(&args.map)?;
    let towncrier = towncrier::read(&ctx.cwd)?;
    let config = migration_config(ctx, towncrier.as_ref())?;
    for (from, target) in &overrides {
        if let Target::Type(to) = target
            && config.find_type(to).is_none()
        {
            return Err(Error::Usage(format!(
                "--map {from}={to}: `{to}` isn't a change type in this package; use one of: {}",
                type_list(&config)
            )));
        }
    }
    // Canonical names for the targets of `--map`.
    let overrides: Vec<(String, Target)> = overrides
        .into_iter()
        .map(|(from, target)| match target {
            Target::Type(t) => (from, Target::Type(config.find_type(&t).map(|t| t.name.clone()).unwrap_or(t))),
            Target::Drop => (from, Target::Drop),
        })
        .collect();

    let mut known_types: Vec<String> = towncrier
        .as_ref()
        .map(|t| t.types.iter().map(|t| t.directory.to_ascii_lowercase()).collect())
        .unwrap_or_default();
    known_types.extend(["feature", "bugfix", "removal", "deprecation", "security", "doc", "misc"].map(String::from));
    known_types.extend(overrides.iter().map(|(from, _)| from.clone()));

    let plan = plan_renames(&config, &known_types, &overrides)?;
    let git = Git::discover(&ctx.cwd)?;

    let mut report = MigrateReport { dry_run: args.dry_run, ..MigrateReport::default() };
    for action in &plan {
        match &action.to {
            Some(to) => {
                let line = format!("{} → {}", show_path(ctx, &action.from), show_path(ctx, to));
                ctx.line(if args.dry_run { format!("rename {line}") } else { format!("Renamed {line}") });
                report.renamed.push(Rename { from: show_path(ctx, &action.from), to: show_path(ctx, to) });
            }
            None => {
                let shown = show_path(ctx, &action.from);
                ctx.line(if args.dry_run { format!("delete {shown}") } else { format!("Deleted {shown}") });
                report.deleted.push(shown);
            }
        }
    }
    if !args.dry_run {
        execute(&plan, git.as_ref())?;
    }
    for action in &plan {
        let Some(to) = &action.to else { continue };
        let source = if args.dry_run { &action.from } else { to };
        let empty = std::fs::read_to_string(source).map(|t| t.trim().is_empty()).unwrap_or(false);
        if empty {
            ctx.warn(format!(
                "{} has no text; vers needs a sentence describing the change before it can release it",
                show_path(ctx, to)
            ));
        }
        if action.from.extension().is_some_and(|e| e == "rst") {
            ctx.warn(format!(
                "{} was written as reStructuredText; vers renders Markdown, so check its formatting",
                show_path(ctx, to)
            ));
        }
    }

    let renamed = report.renamed.len();
    let deleted = report.deleted.len();
    if plan.is_empty() {
        ctx.line(format!("No towncrier fragments to migrate in {}", dir_display(ctx, &config.fragments.directory)));
    } else if args.dry_run {
        ctx.line(format!("Would rename {} and delete {}", count(renamed, "fragment"), count(deleted, "fragment")));
    } else {
        ctx.line(format!("Renamed {} and deleted {}", count(renamed, "fragment"), count(deleted, "fragment")));
    }

    if args.changelog {
        report.changelog = Some(migrate_changelog(ctx, &config.changelog.path, args.dry_run)?);
    }

    if ctx.json_output() {
        ctx.json(&report);
    }
    Ok(())
}

#[derive(Debug, Default, Serialize)]
struct MigrateReport {
    dry_run: bool,
    renamed: Vec<Rename>,
    deleted: Vec<String>,
    changelog: Option<ChangelogReport>,
}

#[derive(Debug, Serialize)]
struct Rename {
    from: String,
    to: String,
}

#[derive(Debug, Serialize)]
struct ChangelogReport {
    path: String,
    changed: bool,
    diff: String,
}

fn count(n: usize, noun: &str) -> String {
    if n == 1 { format!("1 {noun}") } else { format!("{n} {noun}s") }
}

fn dir_display(ctx: &Context, dir: &Path) -> String {
    let shown = show_path(ctx, dir);
    if shown.is_empty() { ".".to_string() } else { shown }
}

fn type_list(config: &Config) -> String {
    config.types.iter().map(|t| t.name.as_str()).collect::<Vec<_>>().join(", ")
}

/// `--map FROM=TO` options. `TO` may be `drop` to delete the fragments.
fn parse_overrides(maps: &[String]) -> Result<Vec<(String, Target)>> {
    let mut out: Vec<(String, Target)> = Vec::new();
    for map in maps {
        let Some((from, to)) = map.split_once('=') else {
            return Err(Error::Usage(format!("--map {map}: expected FROM=TO, for example `misc=changed`")));
        };
        let (from, to) = (from.trim().to_ascii_lowercase(), to.trim());
        if from.is_empty() || to.is_empty() {
            return Err(Error::Usage(format!("--map {map}: expected FROM=TO, for example `misc=changed`")));
        }
        let target = if to.eq_ignore_ascii_case("drop") { Target::Drop } else { Target::Type(to.to_string()) };
        out.retain(|(f, _)| *f != from);
        out.push((from, target));
    }
    Ok(out)
}

/// The config whose fragment directory and types the migration uses: the
/// current package's `vers.toml`, or else towncrier's settings with vers's
/// default types.
fn migration_config(ctx: &Context, towncrier: Option<&TowncrierConfig>) -> Result<Config> {
    let found = match &ctx.config_override {
        Some(path) => Some(path.clone()),
        None => config::find_config_for(&ctx.cwd)?,
    };
    if let Some(path) = found {
        let config = config::load(&path)?;
        if config.is_package() {
            return Ok(config);
        }
    }
    let name = ctx.cwd.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_else(|| "package".into());
    let mut config = Config::default_for(&ctx.cwd, &name);
    if let Some(tc) = towncrier {
        let base = tc.source.parent().unwrap_or(&ctx.cwd);
        config.fragments.directory = join_slash(base, &tc.directory);
        if is_markdown(&tc.filename) {
            config.changelog.path = join_slash(base, &tc.filename);
        }
    } else {
        config.fragments.directory = ctx.cwd.join("newsfragments");
    }
    Ok(config)
}

/// `rel` (a `/`- or `\`-separated path from a towncrier setting) resolved
/// against `base`, lexically. An absolute `rel` is used as it is.
fn join_slash(base: &Path, rel: &str) -> PathBuf {
    if Path::new(rel).is_absolute() {
        return PathBuf::from(rel);
    }
    let mut path = base.to_path_buf();
    for part in rel.split(['/', '\\']).filter(|p| !p.is_empty() && *p != ".") {
        if part == ".." {
            path.pop();
        } else {
            path.push(part);
        }
    }
    path
}

fn is_markdown(filename: &str) -> bool {
    let lower = filename.to_ascii_lowercase();
    lower.ends_with(".md") || lower.ends_with(".markdown")
}

/// One planned change: rename `from` to `to`, or delete it (`to` is `None`).
#[derive(Debug)]
struct Action {
    from: PathBuf,
    to: Option<PathBuf>,
}

/// A towncrier fragment filename: `[+]<name>.<type>[.<counter>][.<ext>]`.
#[derive(Debug)]
struct TcName {
    orphan: bool,
    name: String,
    tc_type: String,
    counter: u32,
}

/// Parse a towncrier filename the way towncrier does: the type is the last
/// dot-separated part (after the first) that is a known type, and a number
/// right after it is the counter.
fn parse_tc_name(file_name: &str, is_type: impl Fn(&str) -> bool) -> Option<TcName> {
    let (orphan, rest) = match file_name.strip_prefix('+') {
        Some(rest) => (true, rest),
        None => (false, file_name),
    };
    let parts: Vec<&str> = rest.split('.').collect();
    for i in (1..parts.len()).rev() {
        if is_type(parts[i]) {
            let counter = parts.get(i + 1).and_then(|c| c.parse::<u32>().ok()).unwrap_or(0);
            return Some(TcName { orphan, name: parts[..i].join("."), tc_type: parts[i].to_string(), counter });
        }
    }
    None
}

/// Sort key of a fragment to move: target name, target type, towncrier
/// counter, current filename.
type MoveKey = (String, String, u32, String);

/// Work out every rename and deletion, without touching the filesystem.
fn plan_renames(config: &Config, known_types: &[String], overrides: &[(String, Target)]) -> Result<Vec<Action>> {
    let dir = &config.fragments.directory;
    let entries = match std::fs::read_dir(dir) {
        Ok(entries) => entries,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(e) => return Err(Error::io(dir, e)),
    };
    let mut files: Vec<String> = Vec::new();
    for entry in entries {
        let entry = entry.map_err(|e| Error::io(dir, e))?;
        if entry.path().is_dir() {
            continue;
        }
        let name = entry.file_name().to_string_lossy().into_owned();
        let lower = name.to_ascii_lowercase();
        if (name.starts_with('.') && !name.starts_with("+.")) || lower.starts_with("readme.") || lower == "readme" {
            continue;
        }
        files.push(name);
    }
    files.sort();

    let is_type = |t: &str| {
        let lower = t.to_ascii_lowercase();
        known_types.contains(&lower) || config.find_type(t).is_some()
    };

    // Files that stay where they are, and files to move: (sort key, from, target).
    let mut occupied: HashSet<String> = HashSet::new();
    let mut moves: Vec<(MoveKey, Option<String>)> = Vec::new();
    let mut unknown: Vec<String> = Vec::new();
    for file in &files {
        let Some(parsed) = parse_tc_name(file, is_type) else {
            occupied.insert(file.clone());
            continue;
        };
        let target = towncrier::target_for(&parsed.tc_type, overrides, config);
        let Some(target) = target else {
            unknown.push(format!("{file} (type `{}`)", parsed.tc_type));
            continue;
        };
        let Target::Type(type_name) = target else {
            moves.push(((String::new(), String::new(), 0, file.clone()), None));
            continue;
        };
        // Already a vers fragment of the right type: leave it alone.
        if !parsed.orphan
            && let Some(vers) = fragment::parse_file_name(file)
            && vers.type_name == type_name
            && vers.name == parsed.name
        {
            occupied.insert(file.clone());
            continue;
        }
        let name = fragment_name(&parsed, config);
        moves.push(((name, type_name.clone(), parsed.counter, file.clone()), Some(type_name)));
    }
    if !unknown.is_empty() {
        return Err(Error::Failed(format!(
            "no vers type for these fragments: {}\n  hint: map them with `--map <towncrier type>=<vers type>` \
             (vers types: {}), or add the types to vers.toml",
            unknown.join(", "),
            type_list(config)
        )));
    }

    // The first fragment of a name and type gets no counter, the next ones
    // `.2`, `.3`, … (as `vers new` does), in towncrier counter order.
    moves.sort_by(|a, b| a.0.cmp(&b.0));
    let mut plan = Vec::new();
    for ((name, type_name, _, file), target) in moves {
        let from = dir.join(&file);
        if target.is_none() {
            plan.push(Action { from, to: None });
            continue;
        }
        let candidate = (1u32..)
            .map(|n| if n == 1 { format!("{name}.{type_name}.md") } else { format!("{name}.{type_name}.{n}.md") })
            .find(|c| !occupied.contains(c))
            .expect("some counter is free");
        occupied.insert(candidate.clone());
        if candidate == file {
            continue;
        }
        plan.push(Action { from, to: Some(dir.join(candidate)) });
    }
    plan.sort_by(|a, b| a.from.cmp(&b.from));
    Ok(plan)
}

/// The vers `<name>` for a towncrier fragment. Orphan names (`+abc`) become
/// slugs; a slug that would be read as a reference gets a `change-` prefix.
fn fragment_name(parsed: &TcName, config: &Config) -> String {
    let name = towncrier::sanitize(&parsed.name);
    if name.is_empty() {
        return "change".to_string();
    }
    let as_ref = fragment::resolve_ref(&name, &config.refs).is_some() || fragment::looks_like_ref(&name);
    if parsed.orphan && as_ref { format!("change-{name}") } else { name }
}

/// Carry out the plan: `git mv`/`git rm` for tracked files, plain filesystem
/// operations otherwise.
fn execute(plan: &[Action], git: Option<&Git>) -> Result<()> {
    let mut pending: Vec<(PathBuf, PathBuf)> = Vec::new();
    for action in plan {
        match &action.to {
            Some(to) => pending.push((action.from.clone(), to.clone())),
            None => remove(&action.from, git)?,
        }
    }
    // Move files whose target is free first; break cycles through a
    // temporary name.
    let mut temp = 0;
    while !pending.is_empty() {
        let index = pending.iter().position(|(_, to)| !to.exists());
        match index {
            Some(i) => {
                let (from, to) = pending.remove(i);
                rename(&from, &to, git)?;
            }
            None => {
                let (from, to) = pending.remove(0);
                let tmp = loop {
                    temp += 1;
                    let tmp = from.with_file_name(format!(".vers-migrate-{temp}.tmp"));
                    if !tmp.exists() {
                        break tmp;
                    }
                };
                rename(&from, &tmp, git)?;
                pending.push((tmp, to));
            }
        }
    }
    Ok(())
}

fn tracked(path: &Path, git: Option<&Git>) -> Result<Option<Git>> {
    match git {
        Some(git) if git.is_tracked(path)? => Ok(Some(git.clone())),
        _ => Ok(None),
    }
}

fn rename(from: &Path, to: &Path, git: Option<&Git>) -> Result<()> {
    match tracked(from, git)? {
        Some(git) => {
            // Root-relative `/` paths, since git runs in the root.
            let args: Vec<OsString> =
                vec!["mv".into(), "--".into(), root_relative(&git, from), root_relative(&git, to)];
            git.run(args)?;
            Ok(())
        }
        None => std::fs::rename(from, to).map_err(|e| Error::io(from, e)),
    }
}

/// `path` relative to the working tree root with `/` separators, or as given
/// when it isn't below the root.
fn root_relative(git: &Git, path: &Path) -> OsString {
    match path.strip_prefix(git.root()) {
        Ok(rel) => rel.components().map(|c| c.as_os_str().to_string_lossy()).collect::<Vec<_>>().join("/").into(),
        Err(_) => path.as_os_str().to_owned(),
    }
}

fn remove(path: &Path, git: Option<&Git>) -> Result<()> {
    match tracked(path, git)? {
        Some(git) => git.rm(&[path.to_path_buf()]),
        None => std::fs::remove_file(path).map_err(|e| Error::io(path, e)),
    }
}

/// `--changelog`: rewrite towncrier release headings and add `## [Unreleased]`.
fn migrate_changelog(ctx: &Context, path: &Path, dry_run: bool) -> Result<ChangelogReport> {
    let shown = show_path(ctx, path);
    let Some(old) = Changelog::load(path)? else {
        ctx.warn(format!("{shown} doesn't exist, so there are no release headings to convert"));
        return Ok(ChangelogReport { path: shown, changed: false, diff: String::new() });
    };
    let old = old.as_str().to_string();
    let mut changelog = Changelog::parse(&headings::convert(&old))?;
    changelog.ensure_unreleased();
    let new = changelog.as_str().to_string();
    if new == old {
        ctx.line(format!("No towncrier release headings to convert in {shown}"));
        return Ok(ChangelogReport { path: shown, changed: false, diff: String::new() });
    }
    let diff = similar::TextDiff::from_lines(&old, &new)
        .unified_diff()
        .header(&format!("a/{shown}"), &format!("b/{shown}"))
        .to_string();
    if !ctx.json_output() && !ctx.quiet {
        print!("{diff}");
    }
    if !dry_run {
        std::fs::write(path, &new).map_err(|e| Error::io(path, e))?;
        ctx.line(format!("Updated {shown}"));
    }
    Ok(ChangelogReport { path: shown, changed: true, diff })
}

/// `vers init --from-towncrier`: called by the init command.
pub fn init_from_towncrier(ctx: &Context, args: &InitArgs) -> Result<()> {
    init::run(ctx, args)
}

/// `path` for display: relative to the current directory (with `..` when it's
/// outside it), with `/` separators.
fn show_path(ctx: &Context, path: &Path) -> String {
    if path.starts_with(&ctx.cwd) {
        return ctx.display_path(path);
    }
    relative_slash(&ctx.cwd, path).unwrap_or_else(|| ctx.display_path(path))
}

/// The path from directory `from` to `to`, with `/` separators. `None` when
/// they share no root (different Windows drives).
fn relative_slash(from: &Path, to: &Path) -> Option<String> {
    let from: Vec<_> = from.components().collect();
    let to_parts: Vec<_> = to.components().collect();
    let common = from.iter().zip(&to_parts).take_while(|(a, b)| a == b).count();
    if common == 0 {
        return None;
    }
    let mut parts: Vec<String> = vec!["..".to_string(); from.len() - common];
    parts.extend(to_parts[common..].iter().map(|c| c.as_os_str().to_string_lossy().into_owned()));
    Some(parts.join("/"))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn types(t: &str) -> bool {
        ["feature", "bugfix", "misc", "added"].contains(&t)
    }

    #[test]
    fn parses_towncrier_names() {
        let p = parse_tc_name("123.feature.md", types).unwrap();
        assert_eq!((p.orphan, p.name.as_str(), p.tc_type.as_str(), p.counter), (false, "123", "feature", 0));
        let p = parse_tc_name("123.feature.1.md", types).unwrap();
        assert_eq!(p.counter, 1);
        let p = parse_tc_name("+abc.bugfix", types).unwrap();
        assert_eq!((p.orphan, p.name.as_str()), (true, "abc"));
        let p = parse_tc_name("a.b.misc.rst", types).unwrap();
        assert_eq!(p.name, "a.b");
        assert!(parse_tc_name("feature", types).is_none());
        assert!(parse_tc_name("notes.txt", types).is_none());
    }

    #[test]
    fn relative_paths() {
        let base = std::env::temp_dir();
        let from = base.join("a").join("b").join("c");
        assert_eq!(relative_slash(&from, &base.join("a").join("vers.toml")).unwrap(), "../../vers.toml");
    }
}
