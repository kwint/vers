//! `vers init`: create a `vers.toml` in the current directory.
//!
//! Spec: `docs/reference/cli.md#vers-init`, `docs/tutorials/first-release.md`,
//! `docs/tutorials/monorepo.md`.

mod detect;

use std::path::{Component, Path, PathBuf};

use serde::Serialize;

use crate::changelog::{Changelog, NEW_CHANGELOG};
use crate::cli::InitArgs;
use crate::config::{self, CONFIG_FILENAMES, RepoKind};
use crate::context::Context;
use crate::error::{Error, Result};
use crate::git::{self, Git};

use detect::{Detected, Locator};

#[derive(Serialize)]
struct JsonOutput {
    config: String,
    shared: bool,
    name: Option<String>,
    extends: Option<String>,
    created: Vec<String>,
    updated: Vec<String>,
    repository: Option<JsonRepo>,
    version_files: Vec<JsonVersionFile>,
}

#[derive(Serialize)]
struct JsonRepo {
    kind: &'static str,
    url: String,
}

#[derive(Serialize)]
struct JsonVersionFile {
    path: String,
    key: Option<String>,
    pattern: Option<String>,
    version: String,
}

pub fn run(ctx: &Context, args: &InitArgs) -> Result<()> {
    let dir = &ctx.cwd;
    if let Some(existing) = CONFIG_FILENAMES.iter().find(|name| dir.join(name).exists()) {
        return Err(Error::Usage(format!(
            "{existing} already exists in this directory; edit it instead of running `vers init` again"
        )));
    }
    if args.from_towncrier {
        return crate::commands::migrate::init_from_towncrier(ctx, args);
    }

    // A config in a parent directory: extend it.
    let parent_config = match dir.parent() {
        Some(parent) => config::find_config_for(parent)?,
        None => None,
    };
    let extends = parent_config.as_deref().map(|p| relative_path(dir, p));

    let name = if args.shared { None } else { Some(package_name(dir, args.name.as_deref())?) };

    // The repository is only detected for a top-level config: an extending
    // config gets it from the config it extends.
    let repo = if args.no_detect || extends.is_some() { None } else { detect_repo(dir) };
    let version_files = if args.no_detect || args.shared { Vec::new() } else { detect::version_files(dir) };

    let text = render_config(extends.as_deref(), name.as_deref(), repo.as_ref(), &version_files);
    let config_path = dir.join("vers.toml");
    write_new(&config_path, &text)?;

    let mut created = vec![ctx.display_path(&config_path)];
    let mut updated = Vec::new();
    let suffix = match (args.shared, &extends) {
        (true, None) => " (shared config, not a package)".to_string(),
        (true, Some(e)) => format!(" (shared config, extends {e})"),
        (false, Some(e)) => format!(" (extends {e})"),
        (false, None) => String::new(),
    };
    ctx.line(format!("Created {}{suffix}", ctx.display_path(&config_path)));

    if !args.shared {
        let changes = dir.join("changes");
        if dir_is_missing_or_empty(&changes) {
            std::fs::create_dir_all(&changes).map_err(|e| Error::io(&changes, e))?;
            let gitkeep = changes.join(".gitkeep");
            write_new(&gitkeep, "")?;
            let shown = ctx.display_path(&gitkeep);
            ctx.line(format!("Created {shown}"));
            created.push(shown);
        }

        let changelog_path = dir.join("CHANGELOG.md");
        match Changelog::load(&changelog_path)? {
            None => {
                write_new(&changelog_path, NEW_CHANGELOG)?;
                let shown = ctx.display_path(&changelog_path);
                ctx.line(format!("Created {shown}"));
                created.push(shown);
            }
            Some(mut changelog) => {
                if changelog.ensure_unreleased() {
                    std::fs::write(&changelog_path, changelog.as_str()).map_err(|e| Error::io(&changelog_path, e))?;
                    let shown = ctx.display_path(&changelog_path);
                    ctx.line(format!("Updated {shown} (added ## [Unreleased])"));
                    updated.push(shown);
                }
            }
        }
    }

    if let Some((kind, url)) = &repo {
        ctx.line(format!("Detected repository: {} ({url})", repo_name(*kind)));
    }
    for file in &version_files {
        let detail = match &file.locator {
            Locator::Key(key) => format!("{key} = {}", file.version),
            Locator::Whole | Locator::Pattern(_) => file.version.to_string(),
        };
        ctx.line(format!("Detected version file: {} ({detail})", file.path));
    }
    if let Some(first) = version_files.first()
        && version_files.iter().any(|f| f.version != first.version)
    {
        let list = version_files.iter().map(|f| format!("{} has {}", f.path, f.version)).collect::<Vec<_>>();
        ctx.warn(format!(
            "the detected version files disagree ({}); set the same version in each before releasing",
            list.join(", ")
        ));
    }

    if ctx.json_output() {
        ctx.json(&JsonOutput {
            config: ctx.display_path(&config_path),
            shared: args.shared,
            name,
            extends,
            created,
            updated,
            repository: repo.map(|(kind, url)| JsonRepo { kind: repo_name(kind), url }),
            version_files: version_files
                .iter()
                .map(|f| JsonVersionFile {
                    path: f.path.clone(),
                    key: match &f.locator {
                        Locator::Key(k) => Some(k.clone()),
                        _ => None,
                    },
                    pattern: match &f.locator {
                        Locator::Pattern(p) => Some(p.clone()),
                        _ => None,
                    },
                    version: f.version.to_string(),
                })
                .collect(),
        });
    }
    Ok(())
}

fn package_name(dir: &Path, name: Option<&str>) -> Result<String> {
    if let Some(name) = name {
        if name.trim().is_empty() || name.chars().any(char::is_whitespace) {
            return Err(Error::Usage(format!(
                "invalid package name \"{name}\": it's used in CLI arguments and tags, so it can't be empty or contain whitespace"
            )));
        }
        return Ok(name.to_string());
    }
    let dir_name = dir.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
    if dir_name.trim().is_empty() || dir_name.chars().any(char::is_whitespace) {
        return Err(Error::Usage(format!(
            "can't use the directory name \"{dir_name}\" as the package name because it's empty or contains whitespace; pass --name <NAME>"
        )));
    }
    Ok(dir_name)
}

fn detect_repo(dir: &Path) -> Option<(RepoKind, String)> {
    let repo = Git::discover(dir).ok().flatten()?;
    let remote = repo.remote_url("origin").ok().flatten()?;
    let url = git::web_url_from_remote(&remote)?;
    match git::detect_repo_kind(&url) {
        RepoKind::None => None,
        kind => Some((kind, url)),
    }
}

fn repo_name(kind: RepoKind) -> &'static str {
    match kind {
        RepoKind::Github => "github",
        RepoKind::Gitlab => "gitlab",
        RepoKind::None => "none",
    }
}

/// The text of the new `vers.toml`: top-level keys, then tables separated by
/// one blank line.
fn render_config(
    extends: Option<&str>,
    name: Option<&str>,
    repo: Option<&(RepoKind, String)>,
    version_files: &[Detected],
) -> String {
    let mut blocks: Vec<String> = Vec::new();
    if let Some(extends) = extends {
        blocks.push(format!("extend = {}\n", basic_string(extends)));
    }
    match name {
        Some(name) => {
            blocks.push(format!("[package]\nname = {}\n", basic_string(name)));
            for file in version_files {
                let mut block = format!("[[version.files]]\npath = {}\n", basic_string(&file.path));
                match &file.locator {
                    Locator::Whole => {}
                    Locator::Key(key) => block.push_str(&format!("key = {}\n", basic_string(key))),
                    Locator::Pattern(p) => block.push_str(&format!("pattern = {}\n", literal_string(p))),
                }
                blocks.push(block);
            }
        }
        None => {
            // A shared config pins the repository so every package extending
            // it links to the same place, wherever it lives.
            if let Some((kind, url)) = repo {
                blocks.push(format!(
                    "[changelog]\nrepo = {}\nrepo_url = {}\n",
                    basic_string(repo_name(*kind)),
                    basic_string(url)
                ));
            }
            blocks.push("[version]\ntag_format = \"{name}@v{version}\"\n".to_string());
        }
    }
    blocks.join("\n")
}

/// A TOML basic string. JSON string escaping is valid TOML.
fn basic_string(s: &str) -> String {
    serde_json::to_string(s).expect("strings serialise")
}

/// A TOML literal string if possible (no escaping, nicer for regexes),
/// otherwise a basic string.
fn literal_string(s: &str) -> String {
    if s.contains('\'') || s.chars().any(|c| c.is_control()) { basic_string(s) } else { format!("'{s}'") }
}

fn write_new(path: &Path, content: &str) -> Result<()> {
    use std::io::Write;
    let mut file =
        std::fs::OpenOptions::new().write(true).create_new(true).open(path).map_err(|e| Error::io(path, e))?;
    file.write_all(content.as_bytes()).map_err(|e| Error::io(path, e))
}

fn dir_is_missing_or_empty(dir: &Path) -> bool {
    match std::fs::read_dir(dir) {
        Ok(mut entries) => entries.next().is_none(),
        Err(_) => !dir.exists(),
    }
}

/// `target` (a file) relative to directory `from`, with forward slashes.
fn relative_path(from: &Path, target: &Path) -> String {
    let from: Vec<Component> = from.components().collect();
    let to: Vec<Component> = target.components().collect();
    let common = from.iter().zip(&to).take_while(|(a, b)| a == b).count();
    if common == 0 {
        // Different roots (e.g. drives): an absolute path is the only option.
        return target.to_string_lossy().replace('\\', "/");
    }
    let mut parts: Vec<String> = Vec::new();
    for _ in common..from.len() {
        parts.push("..".to_string());
    }
    let rest: PathBuf = to[common..].iter().collect();
    parts.push(rest.to_string_lossy().replace('\\', "/"));
    parts.join("/")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn relative_paths() {
        let base = std::env::temp_dir();
        let from = base.join("a").join("b").join("c");
        assert_eq!(relative_path(&from, &base.join("a").join("vers.toml")), "../../vers.toml");
        assert_eq!(relative_path(&from, &base.join("a").join("b").join("vers.toml")), "../vers.toml");
        assert_eq!(relative_path(&from, &base.join("a").join("x").join(".vers.toml")), "../../x/.vers.toml");
    }

    #[test]
    fn strings() {
        assert_eq!(basic_string("a\"b\\c"), r#""a\"b\\c""#);
        assert_eq!(literal_string(r"^v = (?<version>\S+)"), r"'^v = (?<version>\S+)'");
        assert_eq!(literal_string("a'b"), "\"a'b\"");
    }
}
