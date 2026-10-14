//! `vers init --from-towncrier`: build a `vers.toml` from `[tool.towncrier]`.
//!
//! Spec: `docs/how-to/migrate-from-towncrier.md` step 1 and
//! `docs/reference/cli.md#vers-init`. Output and JSON follow plain `vers init`.

use std::path::Path;

use serde::Serialize;

use crate::changelog::{Changelog, NEW_CHANGELOG};
use crate::cli::InitArgs;
use crate::config::{self, CONFIG_FILENAMES, RepoKind};
use crate::context::Context;
use crate::error::{Error, Result};
use crate::git::{self, Git};

use super::towncrier::{self, TowncrierConfig};
use super::{is_markdown, join_slash, relative_slash, show_path};

#[derive(Serialize)]
struct JsonOutput {
    config: String,
    shared: bool,
    name: String,
    extends: Option<String>,
    created: Vec<String>,
    updated: Vec<String>,
    repository: Option<JsonRepo>,
    version_files: Vec<JsonVersionFile>,
    fragments_directory: String,
    changelog: String,
    custom_types: Vec<String>,
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

/// What goes into the new `vers.toml`.
struct Plan<'a> {
    extends: Option<&'a str>,
    name: &'a str,
    tc: &'a TowncrierConfig,
    changelog: &'a str,
    customs: &'a [(String, String)],
    version_file: Option<&'a (String, String)>,
    /// Write `[changelog] path` / `[fragments] directory` even when they're
    /// the defaults, because an extended config would otherwise change them.
    explicit_changelog: bool,
    explicit_directory: bool,
}

pub fn run(ctx: &Context, args: &InitArgs) -> Result<()> {
    let dir = &ctx.cwd;
    if let Some(existing) = CONFIG_FILENAMES.iter().find(|name| dir.join(name).exists()) {
        return Err(Error::Usage(format!(
            "{existing} already exists in this directory; edit it instead of running `vers init` again"
        )));
    }
    if args.shared {
        return Err(Error::Usage(
            "--from-towncrier creates a package config, so it can't be used with --shared".into(),
        ));
    }
    let tc = towncrier::read(dir)?.ok_or_else(|| {
        Error::Usage("no [tool.towncrier] table found in pyproject.toml or towncrier.toml in this directory".into())
    })?;
    let name = package_name(dir, args.name.as_deref())?;

    let parent_config = match dir.parent() {
        Some(parent) => config::find_config_for(parent)?,
        None => None,
    };
    let extends = parent_config
        .as_ref()
        .map(|path| relative_slash(dir, path).unwrap_or_else(|| path.to_string_lossy().replace('\\', "/")));

    let changelog = if is_markdown(&tc.filename) {
        tc.filename.replace('\\', "/")
    } else {
        ctx.warn(format!(
            "towncrier writes {}, but vers only writes Markdown; using CHANGELOG.md instead",
            tc.filename
        ));
        "CHANGELOG.md".to_string()
    };
    let customs: Vec<(String, String)> = tc.types.iter().filter_map(towncrier::custom_type).collect();
    // As in `vers init`, the repository is only detected for a top-level config.
    let repo = if args.no_detect || extends.is_some() { None } else { detect_repo(dir) };
    let version_file = if args.no_detect { None } else { pyproject_version(ctx, dir)? };

    let mut plan = Plan {
        extends: extends.as_deref(),
        name: &name,
        tc: &tc,
        changelog: &changelog,
        customs: &customs,
        version_file: version_file.as_ref(),
        explicit_changelog: false,
        explicit_directory: false,
    };
    let expected_directory = join_slash(dir, &tc.directory);
    let expected_changelog = join_slash(dir, &changelog);

    // Write the config, then load it (which validates it and everything it
    // extends) and the existing changelog before anything else is written,
    // so a failure leaves nothing behind. If an extended config moves the
    // fragment directory or changelog, write them out explicitly.
    let config_path = dir.join("vers.toml");
    let (config, existing_changelog) = loop {
        write_new(&config_path, &render(&plan))?;
        let loaded = config::load(&config_path).and_then(|config| {
            let changelog = Changelog::load(&config.changelog.path)?;
            Ok((config, changelog))
        });
        let (config, existing) = match loaded {
            Ok(loaded) => loaded,
            Err(e) => {
                let _ = std::fs::remove_file(&config_path);
                return Err(e);
            }
        };
        let wrong_directory = config.fragments.directory != expected_directory && !plan.explicit_directory;
        let wrong_changelog = config.changelog.path != expected_changelog && !plan.explicit_changelog;
        if !wrong_directory && !wrong_changelog {
            break (config, existing);
        }
        std::fs::remove_file(&config_path).map_err(|e| Error::io(&config_path, e))?;
        plan.explicit_directory |= wrong_directory;
        plan.explicit_changelog |= wrong_changelog;
    };

    let mut created = vec![show_path(ctx, &config_path)];
    let mut updated = Vec::new();
    match &extends {
        Some(e) => ctx.line(format!("Created {} (extends {e})", show_path(ctx, &config_path))),
        None => ctx.line(format!("Created {}", show_path(ctx, &config_path))),
    }

    let fragments_dir = &config.fragments.directory;
    if dir_is_missing_or_empty(fragments_dir) {
        std::fs::create_dir_all(fragments_dir).map_err(|e| Error::io(fragments_dir, e))?;
        let gitkeep = fragments_dir.join(".gitkeep");
        write_new(&gitkeep, "")?;
        let shown = show_path(ctx, &gitkeep);
        ctx.line(format!("Created {shown}"));
        created.push(shown);
    }

    let changelog_path = &config.changelog.path;
    match existing_changelog {
        None => {
            if let Some(parent) = changelog_path.parent() {
                std::fs::create_dir_all(parent).map_err(|e| Error::io(parent, e))?;
            }
            write_new(changelog_path, NEW_CHANGELOG)?;
            let shown = show_path(ctx, changelog_path);
            ctx.line(format!("Created {shown}"));
            created.push(shown);
        }
        Some(mut changelog) => {
            if changelog.ensure_unreleased() {
                std::fs::write(changelog_path, changelog.as_str()).map_err(|e| Error::io(changelog_path, e))?;
                let shown = show_path(ctx, changelog_path);
                ctx.line(format!("Updated {shown} (added ## [Unreleased])"));
                updated.push(shown);
            }
        }
    }

    if let Some((kind, url)) = &repo {
        ctx.line(format!("Detected repository: {} ({url})", repo_name(*kind)));
    }
    if let Some((key, version)) = &version_file {
        ctx.line(format!("Detected version file: pyproject.toml ({key} = {version})"));
    }
    for (type_name, heading) in &customs {
        ctx.line(format!("Added custom type: {type_name} ({heading})"));
    }
    ctx.line("Next, rename existing fragments with `vers migrate towncrier --dry-run`");

    if ctx.json_output() {
        let shown_dir = show_path(ctx, fragments_dir);
        ctx.json(&JsonOutput {
            config: show_path(ctx, &config_path),
            shared: false,
            name: name.clone(),
            extends,
            created,
            updated,
            repository: repo.map(|(kind, url)| JsonRepo { kind: repo_name(kind), url }),
            version_files: version_file
                .iter()
                .map(|(key, version)| JsonVersionFile {
                    path: "pyproject.toml".into(),
                    key: Some(key.clone()),
                    pattern: None,
                    version: version.clone(),
                })
                .collect(),
            fragments_directory: if shown_dir.is_empty() { ".".into() } else { shown_dir },
            changelog: show_path(ctx, changelog_path),
            custom_types: customs.iter().map(|(n, _)| n.clone()).collect(),
        });
    }
    Ok(())
}

/// The text of the new `vers.toml`: top-level keys, then tables separated by
/// one blank line.
fn render(plan: &Plan) -> String {
    let mut blocks: Vec<String> = Vec::new();
    if let Some(extends) = plan.extends {
        blocks.push(format!("extend = {}\n", quote(extends)));
    }
    blocks.push(format!("[package]\nname = {}\n", quote(plan.name)));
    if plan.changelog != "CHANGELOG.md" || plan.explicit_changelog {
        blocks.push(format!("[changelog]\npath = {}\n", quote(plan.changelog)));
    }
    if plan.tc.directory != "changes" || plan.explicit_directory {
        blocks.push(format!("[fragments]\ndirectory = {}\n", quote(&plan.tc.directory)));
    }
    if !plan.customs.is_empty() {
        // `[[types]]` replaces the whole default list, so the defaults are
        // written out too.
        for t in config::default_types() {
            let mut block = format!("[[types]]\nname = {}\nheading = {}\n", quote(&t.name), quote(&t.heading));
            if !t.aliases.is_empty() {
                let aliases: Vec<String> = t.aliases.iter().map(|a| quote(a)).collect();
                block.push_str(&format!("aliases = [{}]\n", aliases.join(", ")));
            }
            block.push_str(&format!("bump = {}\n", quote(&t.bump.to_string())));
            blocks.push(block);
        }
        for (type_name, heading) in plan.customs {
            blocks.push(format!(
                "[[types]]\nname = {}\nheading = {}\nbump = \"patch\"\n",
                quote(type_name),
                quote(heading)
            ));
        }
    }
    if let Some((key, _)) = plan.version_file {
        blocks.push(format!("[[version.files]]\npath = \"pyproject.toml\"\nkey = {}\n", quote(key)));
    }
    blocks.join("\n")
}

/// The package name: `--name`, else the directory name. Same rules as `vers init`.
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

/// `project.version` or `tool.poetry.version` in `pyproject.toml`, as
/// `(key, version)`. A dynamic or non-SemVer version is skipped.
fn pyproject_version(ctx: &Context, dir: &Path) -> Result<Option<(String, String)>> {
    let Some(table) = towncrier::read_toml(&dir.join("pyproject.toml"))? else { return Ok(None) };
    for key in ["project.version", "tool.poetry.version"] {
        let mut value: Option<&toml::Value> = None;
        let mut table_ref = Some(&table);
        let parts: Vec<&str> = key.split('.').collect();
        for (i, part) in parts.iter().enumerate() {
            let Some(t) = table_ref else { break };
            let v = t.get(*part);
            if i == parts.len() - 1 {
                value = v;
            } else {
                table_ref = v.and_then(toml::Value::as_table);
            }
        }
        let Some(version) = value.and_then(toml::Value::as_str) else { continue };
        if semver::Version::parse(version).is_err() {
            ctx.warn(format!(
                "pyproject.toml has version `{version}` ({key}), which isn't a SemVer version, so it isn't added as a version file"
            ));
            return Ok(None);
        }
        return Ok(Some((key.to_string(), version.to_string())));
    }
    Ok(None)
}

/// A TOML basic string. JSON string escaping is valid TOML.
fn quote(s: &str) -> String {
    serde_json::to_string(s).expect("strings serialise")
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
