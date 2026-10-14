//! `vers release`: write the changelog, update the version files, remove the
//! fragments and optionally commit and tag.
//!
//! Spec: `docs/reference/cli.md#vers-release`, `docs/concepts/versioning.md`,
//! `docs/how-to/commit-and-tag-releases.md`, `DESIGN.md` §6.1.
//!
//! Every change for every selected package is computed in memory first. Files
//! are only written once all packages have been prepared successfully, so a
//! failing package leaves the working tree untouched.

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

use semver::Version;
use serde::Serialize;

use crate::changelog::{self, Changelog};
use crate::cli::ReleaseArgs;
use crate::cli::VersionChoiceArgs;
use crate::config::{self, Config, Selection};
use crate::context::Context;
use crate::error::{Error, Result};
use crate::git::Git;
use crate::plan::{self, PackagePlan};
use crate::version::{self, ChooseInput, VersionSource};

/// A file whose new contents have been computed but not yet written.
struct FileChange {
    path: PathBuf,
    /// Current contents, or `None` for a new file.
    old: Option<String>,
    new: String,
}

struct VersionFileChange {
    change: FileChange,
    old_version: Version,
}

/// One package's release, prepared in memory.
struct Prepared {
    name: String,
    version: Version,
    source: String,
    changelog: FileChange,
    added_unreleased: bool,
    version_files: Vec<VersionFileChange>,
    fragments: Vec<PathBuf>,
    /// The tag to create, when this package tags.
    tag: Option<String>,
    commit_message: String,
}

#[derive(Serialize)]
struct JsonOutput {
    released: Vec<JsonRelease>,
    skipped: Vec<String>,
    dry_run: bool,
}

#[derive(Serialize)]
struct JsonRelease {
    name: String,
    version: String,
    source: String,
    files: Vec<String>,
    removed: Vec<String>,
    tag: Option<String>,
}

pub fn run(ctx: &Context, args: &ReleaseArgs) -> Result<()> {
    let distinct: BTreeSet<&String> = args.packages.iter().collect();
    if args.choice.version.is_some() && (args.all || distinct.len() > 1) {
        return Err(Error::Usage(
            "--version can't be used with --all or with more than one package; use --bump, or release the \
             packages one at a time"
                .to_string(),
        ));
    }

    let selection = if args.all {
        Selection::All
    } else if !args.packages.is_empty() {
        Selection::Named(args.packages.clone())
    } else {
        Selection::Current
    };
    let packages = config::select_packages(&ctx.cwd, &selection, ctx.config_override.as_deref())?;
    let git = Git::discover(&ctx.cwd)?;

    // 1. Plan and prepare every package in memory.
    let mut prepared: Vec<Prepared> = Vec::new();
    let mut skipped: Vec<String> = Vec::new();
    // Packages and skips in selection order, for the text output.
    let mut order: Vec<Result<usize, String>> = Vec::new();
    for config in &packages {
        let plan = plan_package(config, git.as_ref(), &args.choice, args.force)?;
        for warning in &plan.warnings {
            ctx.warn(warning);
        }
        if plan.next.is_none() {
            if args.all {
                skipped.push(config.name().to_string());
                order.push(Err(config.name().to_string()));
                continue;
            }
            return Err(Error::Failed(format!("nothing to release for `{}`: no pending fragments", config.name())));
        }
        let tag = match (args.tag, args.no_tag) {
            (true, _) => true,
            (_, true) => false,
            _ => config.version.tag,
        };
        order.push(Ok(prepared.len()));
        prepared.push(prepare(&plan, tag)?);
    }

    // 2. Checks that need every package: no conflicting files or tags, git for
    //    tagging, no existing tags.
    check_conflicts(ctx, &prepared)?;
    let tagging: Vec<&Prepared> = prepared.iter().filter(|p| p.tag.is_some()).collect();
    if !tagging.is_empty() {
        let Some(git) = git.as_ref() else {
            return Err(Error::Usage(
                "tagging a release needs a git repository, but the current directory isn't in one; pass --no-tag \
                 or set `[version] tag = false`"
                    .to_string(),
            ));
        };
        let existing = git.tags()?;
        for p in &tagging {
            let tag = p.tag.as_deref().expect("tagging");
            if existing.iter().any(|t| t == tag) {
                return Err(Error::Failed(format!(
                    "tag `{tag}` already exists; delete it with `git tag -d {tag}` or release a different version"
                )));
            }
        }
    }

    if args.dry_run {
        print_dry_run(ctx, &prepared);
        if ctx.json_output() {
            ctx.json(&json_output(ctx, &prepared, skipped, true));
        }
        return Ok(());
    }

    // 3. Write everything.
    let mut committed: Vec<PathBuf> = Vec::new();
    for p in &prepared {
        write_file(&p.changelog)?;
        for vf in &p.version_files {
            write_file(&vf.change)?;
        }
        let in_head = remove_fragments(git.as_ref(), &p.fragments, p.tag.is_some())?;
        if p.tag.is_some() {
            committed.push(p.changelog.path.clone());
            committed.extend(p.version_files.iter().map(|vf| vf.change.path.clone()));
            committed.extend(in_head);
        }
    }

    // 4. Commit and tag.
    let mut commit_sha: Option<String> = None;
    if !tagging.is_empty() {
        let git = git.as_ref().expect("checked above");
        let message = match tagging.as_slice() {
            [only] => only.commit_message.clone(),
            many => {
                let lines = many.iter().map(|p| p.commit_message.as_str()).collect::<Vec<_>>().join("\n");
                format!("Release {} packages\n\n{lines}", many.len())
            }
        };
        let written = "the release files were written, but";
        git.commit_paths(&committed, &message)
            .map_err(|e| Error::Failed(format!("{written} committing failed: {e}")))?;
        for p in &tagging {
            let tag = p.tag.as_deref().expect("tagging");
            git.tag(tag, &p.commit_message)
                .map_err(|e| Error::Failed(format!("{written} creating tag `{tag}` failed: {e}")))?;
        }
        commit_sha = Some(git.run(["rev-parse", "--short", "HEAD"])?);
    }

    // 5. Report.
    if ctx.json_output() {
        ctx.json(&json_output(ctx, &prepared, skipped, false));
        return Ok(());
    }
    for entry in order {
        match entry {
            Err(name) => ctx.line(format!("Skipped {name} (no fragments)")),
            Ok(i) => {
                let p = &prepared[i];
                ctx.line(format!("Releasing {} {} ({})", p.name, p.version, p.source));
                let changelog_path = show(ctx, &p.changelog.path);
                if p.added_unreleased {
                    ctx.line(format!("  Added ## [Unreleased] to {changelog_path}"));
                }
                let verb = if p.changelog.old.is_some() { "Updated" } else { "Created" };
                ctx.line(format!("  {verb} {changelog_path}"));
                for vf in &p.version_files {
                    ctx.line(format!("  Updated {} ({} → {})", show(ctx, &vf.change.path), vf.old_version, p.version));
                }
                match p.fragments.len() {
                    0 => {}
                    1 => ctx.line("  Removed 1 fragment"),
                    n => ctx.line(format!("  Removed {n} fragments")),
                }
                if let (Some(tag), Some(sha)) = (&p.tag, &commit_sha) {
                    ctx.line(format!("  Committed {sha}"));
                    ctx.line(format!("  Tagged {tag}"));
                }
            }
        }
    }
    Ok(())
}

/// [`plan::plan_package`], except that version files above `[version]
/// initial` count as the current version when the changelog and tags have no
/// release (the monorepo tutorial: `Cargo.toml` at 1.4.0 → release 1.5.0),
/// while files at the initial version are released as they are (the first
/// release tutorial: `VERSION` at 0.1.0 → release 0.1.0).
///
/// TODO: private workaround; this belongs in `plan::plan_package` so `status`,
/// `draft` and `version` agree.
fn plan_package(config: &Config, git: Option<&Git>, choice: &VersionChoiceArgs, force: bool) -> Result<PackagePlan> {
    let mut plan = plan::plan_package(config, git, choice, force)?;
    let Some((files, _)) = plan.files_version.clone() else { return Ok(plan) };
    if plan.current.is_some() || !files.cmp_precedence(&config.version.initial).is_gt() {
        return Ok(plan);
    }
    if let Some(next) = &plan.next {
        plan.warnings.retain(|w| !next.warnings.contains(w));
    }
    let has_visible = plan.visible_fragments().next().is_some();
    let finalising = choice.pre.is_none() && !files.pre.is_empty();
    plan.current = Some(files.clone());
    plan.next = if has_visible || finalising {
        let next = version::choose(&ChooseInput {
            current: Some(files),
            files_version: plan.files_version.clone(),
            cli_version: choice.version.clone(),
            cli_bump: choice.bump,
            pre: choice.pre.clone(),
            fragments_bump: version::infer_bump(&plan.fragments, &config.types),
            settings: &config.version,
        })?;
        plan.warnings.extend(next.warnings.iter().cloned());
        Some(next)
    } else {
        None
    };
    Ok(plan)
}

/// Compute a package's changelog, version file contents and fragments to
/// remove, without writing anything.
fn prepare(plan: &PackagePlan, tag: bool) -> Result<Prepared> {
    let config = &plan.config;
    let next = plan.next.as_ref().expect("caller checked");
    let version = next.next.clone();
    let release = crate::commands::draft::build_release_for(plan)?
        .expect("a plan with a next version always has a release section");

    let old_changelog = plan.changelog.as_ref().map(|c| c.as_str().to_string());
    let (mut changelog, added_unreleased) = match &plan.changelog {
        Some(existing) => {
            let mut changelog = existing.clone();
            let added = changelog.ensure_unreleased();
            (changelog, added)
        }
        None => (Changelog::parse(changelog::NEW_CHANGELOG)?, false),
    };
    changelog.insert_release(&release, config)?;

    let mut version_files = Vec::new();
    for spec in &config.version.files {
        let current = version::read_file_version(spec)?;
        if current == version {
            continue;
        }
        let old = crate::error::read_to_string(&spec.path)?;
        let new = version::update_file_version(spec, &version)?;
        version_files.push(VersionFileChange {
            change: FileChange { path: spec.path.clone(), old: Some(old), new },
            old_version: current,
        });
    }

    let name = config.name().to_string();
    let render = |template: &str| template.replace("{name}", &name).replace("{version}", &version.to_string());
    Ok(Prepared {
        source: source_text(plan, &next.source, config),
        changelog: FileChange {
            path: config.changelog.path.clone(),
            old: old_changelog,
            new: changelog.as_str().to_string(),
        },
        added_unreleased,
        version_files,
        fragments: plan.fragments.iter().map(|f| f.path.clone()).collect(),
        tag: tag.then(|| changelog::tag_name(&config.version.tag_format, &name, &version)),
        commit_message: render(&config.version.commit_message),
        name,
        version,
    })
}

/// How the version was chosen, as shown in `Releasing <name> <version> (<source>)`.
fn source_text(plan: &PackagePlan, source: &VersionSource, config: &Config) -> String {
    match source {
        VersionSource::Cli => "from command line".to_string(),
        VersionSource::Files(path) => {
            format!("from {}", relative_path(path, &config.dir))
        }
        VersionSource::Inferred(_) if plan.current.is_none() => "initial".to_string(),
        VersionSource::Inferred(bump) => format!("inferred: {bump}"),
        VersionSource::FinalisePrerelease => "finalising pre-release".to_string(),
    }
}

/// `path` for display: relative to the current directory, using `..` when it's
/// outside it (a package's changelog seen from a subdirectory, a workspace
/// version file), always with `/`.
fn show(ctx: &Context, path: &Path) -> String {
    relative_path(path, &ctx.cwd)
}

/// `path` relative to `base`, with `..` components where needed and `/`
/// separators. Falls back to the path as given when the two share no root
/// (such as different drives on Windows).
///
/// TODO: `context::display_relative` shows paths outside `base` as absolute
/// paths; this belongs there.
fn relative_path(path: &Path, base: &Path) -> String {
    let path_parts: Vec<_> = path.components().collect();
    let base_parts: Vec<_> = base.components().collect();
    let root = root_len(&path_parts);
    if !path.is_absolute() || root == 0 || path_parts.get(..root) != base_parts.get(..root) {
        return crate::context::display_relative(path, base);
    }
    let common = path_parts.iter().zip(&base_parts).take_while(|(a, b)| a == b).count();
    let mut parts: Vec<String> = base_parts[common..].iter().map(|_| "..".to_string()).collect();
    parts.extend(path_parts[common..].iter().map(|c| c.as_os_str().to_string_lossy().into_owned()));
    if parts.is_empty() { ".".to_string() } else { parts.join("/") }
}

/// Number of leading components that make up the root (`/`, or `C:` + `\`).
fn root_len(parts: &[std::path::Component]) -> usize {
    parts.iter().take_while(|c| matches!(c, std::path::Component::Prefix(_) | std::path::Component::RootDir)).count()
}

/// Checks across packages that the per-package preparation can't make: two
/// packages must not change the same file to different contents (a shared
/// workspace version file released at two versions), and must not create the
/// same tag (a `tag_format` without `{name}`).
fn check_conflicts(ctx: &Context, prepared: &[Prepared]) -> Result<()> {
    let mut seen: Vec<(&Path, &str, &str)> = Vec::new();
    for p in prepared {
        let mut changes = vec![&p.changelog];
        changes.extend(p.version_files.iter().map(|vf| &vf.change));
        for change in changes {
            match seen.iter().find(|(path, _, _)| *path == change.path) {
                Some((_, _, new)) if *new == change.new => {}
                Some((path, other, _)) => {
                    return Err(Error::Failed(format!(
                        "`{other}` and `{}` would both change {} in different ways; release them one at a time",
                        p.name,
                        show(ctx, path)
                    )));
                }
                None => seen.push((&change.path, &p.name, &change.new)),
            }
        }
    }
    let mut tags: Vec<(&str, &str)> = Vec::new();
    for p in prepared {
        let Some(tag) = p.tag.as_deref() else { continue };
        if let Some((_, other)) = tags.iter().find(|(t, _)| *t == tag) {
            return Err(Error::Failed(format!(
                "`{other}` and `{}` would both be tagged `{tag}`; add `{{name}}` to `[version] tag_format`, \
                 e.g. \"{{name}}@v{{version}}\"",
                p.name
            )));
        }
        tags.push((tag, &p.name));
    }
    Ok(())
}

fn write_file(change: &FileChange) -> Result<()> {
    if change.old.as_deref() == Some(change.new.as_str()) {
        return Ok(());
    }
    if let Some(parent) = change.path.parent() {
        std::fs::create_dir_all(parent).map_err(|e| Error::io(parent, e))?;
    }
    std::fs::write(&change.path, &change.new).map_err(|e| Error::io(&change.path, e))
}

/// Delete the fragment files: tracked ones with `git rm` (so the deletion is
/// staged), others from disk. When `for_commit`, returns the fragments that
/// exist in `HEAD`, whose deletions belong in the release commit.
fn remove_fragments(git: Option<&Git>, fragments: &[PathBuf], for_commit: bool) -> Result<Vec<PathBuf>> {
    let mut tracked = Vec::new();
    let mut in_head = Vec::new();
    for path in fragments {
        if let Some(git) = git {
            if for_commit && exists_in_head(git, path) {
                in_head.push(path.clone());
            }
            if git.is_tracked(path)? {
                tracked.push(path.clone());
            }
        }
        // Removing the file first lets `git rm` stage the deletion even when the
        // fragment has staged or unstaged edits.
        match std::fs::remove_file(path) {
            Ok(()) => {}
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
            Err(e) => return Err(Error::io(path, e)),
        }
    }
    if let Some(git) = git {
        git.rm(&tracked)?;
    }
    Ok(in_head)
}

fn exists_in_head(git: &Git, path: &Path) -> bool {
    let rel = crate::context::display_relative(path, git.root());
    git.run(["cat-file", "-e", &format!("HEAD:{rel}")]).is_ok()
}

fn print_dry_run(ctx: &Context, prepared: &[Prepared]) {
    // A file shared by several packages (a workspace version file) is shown once.
    let mut shown_paths: Vec<&Path> = Vec::new();
    for p in prepared {
        let mut changes = vec![&p.changelog];
        changes.extend(p.version_files.iter().map(|vf| &vf.change));
        for change in changes {
            let old = change.old.as_deref().unwrap_or("");
            if old == change.new || shown_paths.contains(&change.path.as_path()) {
                continue;
            }
            shown_paths.push(&change.path);
            let shown = show(ctx, &change.path);
            let from = if change.old.is_some() { format!("a/{shown}") } else { "/dev/null".to_string() };
            let diff = similar::TextDiff::from_lines(old, change.new.as_str())
                .unified_diff()
                .context_radius(3)
                .header(&from, &format!("b/{shown}"))
                .to_string();
            ctx.line(diff.trim_end_matches('\n'));
        }
        for fragment in &p.fragments {
            ctx.line(format!("delete {}", show(ctx, fragment)));
        }
    }
}

fn json_output(ctx: &Context, prepared: &[Prepared], skipped: Vec<String>, dry_run: bool) -> JsonOutput {
    let released = prepared
        .iter()
        .map(|p| {
            let mut files = vec![show(ctx, &p.changelog.path)];
            files.extend(p.version_files.iter().map(|vf| show(ctx, &vf.change.path)));
            JsonRelease {
                name: p.name.clone(),
                version: p.version.to_string(),
                source: p.source.clone(),
                files,
                removed: p.fragments.iter().map(|f| show(ctx, f)).collect(),
                tag: p.tag.clone(),
            }
        })
        .collect();
    JsonOutput { released, skipped, dry_run }
}
