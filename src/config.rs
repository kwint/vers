//! Configuration: discovering `vers.toml` files, resolving `extend` chains and
//! presets, and selecting packages.
//!
//! Spec: `docs/concepts/monorepos.md`, `docs/reference/configuration.md`.
//!
//! The types in this module are the *resolved* configuration: every path is
//! absolute, presets are applied, `extend` has been followed, and defaults
//! are filled in. Other modules only ever see a resolved [`Config`].

use std::path::{Path, PathBuf};

use regex::Regex;
use semver::{Version, VersionReq};

use crate::error::{Error, Result};
use crate::version::Bump;

mod resolve;

/// Config file names, in order of precedence within one directory.
pub const CONFIG_FILENAMES: [&str; 2] = [".vers.toml", "vers.toml"];

/// A fully resolved config file.
#[derive(Debug, Clone)]
pub struct Config {
    /// Absolute path of the config file this was loaded from.
    pub path: PathBuf,
    /// Directory containing [`Config::path`].
    pub dir: PathBuf,
    pub required_version: Option<VersionReq>,
    /// `None` for a shared config (no `[package]` table).
    pub package: Option<PackageSettings>,
    pub changelog: ChangelogSettings,
    pub fragments: FragmentSettings,
    /// Change types in changelog order.
    pub types: Vec<ChangeType>,
    /// Reference kinds in matching order: preset kinds first, then configured ones.
    pub refs: Vec<RefKind>,
    pub version: VersionSettings,
    pub check: CheckSettings,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PackageSettings {
    pub name: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RepoKind {
    Github,
    Gitlab,
    None,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum RefPosition {
    #[default]
    End,
    Start,
}

#[derive(Debug, Clone)]
pub struct ChangelogSettings {
    /// Absolute path of the changelog file.
    pub path: PathBuf,
    /// The resolved repository kind (`auto` has already been resolved).
    pub repo: RepoKind,
    /// Web URL of the repository, without a trailing slash.
    pub repo_url: Option<String>,
    /// Compare link template with `{from}` and `{to}`; `{repo_url}` already substituted.
    /// `None` means no links are written.
    pub compare_url: Option<String>,
    /// First-release link template with `{tag}`; `{repo_url}` already substituted.
    pub release_url: Option<String>,
    pub ref_position: RefPosition,
    pub breaking_prefix: String,
}

#[derive(Debug, Clone)]
pub struct FragmentSettings {
    /// Absolute path of the fragment directory.
    pub directory: PathBuf,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ChangeType {
    pub name: String,
    pub heading: String,
    pub aliases: Vec<String>,
    pub bump: Bump,
    pub hidden: bool,
}

#[derive(Debug, Clone)]
pub struct RefKind {
    pub name: String,
    /// Matches the whole fragment name and has a named group `id`.
    pub matcher: Regex,
    /// Link text template with `{id}`.
    pub display: String,
    /// Link target template with `{id}`; `{repo_url}` already substituted.
    /// `None` when the template needs a `repo_url` that isn't known.
    pub url: Option<String>,
}

#[derive(Debug, Clone)]
pub struct VersionSettings {
    pub initial: Version,
    pub zero_major_shift: bool,
    pub fold_prereleases: bool,
    pub tag: bool,
    /// Tag template with `{name}` and `{version}`.
    pub tag_format: String,
    /// Commit message template with `{name}` and `{version}`.
    pub commit_message: String,
    pub files: Vec<VersionFileSpec>,
}

#[derive(Debug, Clone)]
pub struct VersionFileSpec {
    /// Absolute path of the version file.
    pub path: PathBuf,
    pub locator: VersionLocator,
}

#[derive(Debug, Clone)]
pub enum VersionLocator {
    /// The whole file, trimmed, is the version.
    Whole,
    /// A dotted key path in a TOML, JSON or YAML file.
    Key(String),
    /// A regex with a named group `version` that matches exactly once.
    Pattern(Regex),
}

#[derive(Debug, Clone)]
pub struct CheckSettings {
    pub require_fragment: bool,
    /// Globs relative to [`Config::dir`].
    pub ignore: Vec<String>,
}

impl Config {
    /// A config with every default applied, as if `dir/vers.toml` contained
    /// only `[package] name = <name>` and `repo = "none"`. Intended for tests
    /// and for modules that need a config without touching the filesystem.
    pub fn default_for(dir: &Path, name: &str) -> Config {
        Config {
            path: dir.join("vers.toml"),
            dir: dir.to_path_buf(),
            required_version: None,
            package: Some(PackageSettings { name: name.to_string() }),
            changelog: ChangelogSettings {
                path: dir.join("CHANGELOG.md"),
                repo: RepoKind::None,
                repo_url: None,
                compare_url: None,
                release_url: None,
                ref_position: RefPosition::End,
                breaking_prefix: "**Breaking:** ".to_string(),
            },
            fragments: FragmentSettings { directory: dir.join("changes") },
            types: default_types(),
            refs: Vec::new(),
            version: VersionSettings {
                initial: Version::new(0, 1, 0),
                zero_major_shift: true,
                fold_prereleases: true,
                tag: false,
                tag_format: "v{version}".to_string(),
                commit_message: "Release {name} {version}".to_string(),
                files: Vec::new(),
            },
            check: CheckSettings { require_fragment: true, ignore: Vec::new() },
        }
    }

    /// The same config with a repository preset applied for `repo_url`.
    pub fn with_repo(mut self, repo: RepoKind, repo_url: &str) -> Config {
        let repo_url = repo_url.trim_end_matches('/').to_string();
        let preset = Preset::for_kind(repo);
        self.changelog.repo = repo;
        self.changelog.compare_url = preset.compare_url.map(|t| t.replace("{repo_url}", &repo_url));
        self.changelog.release_url = preset.release_url.map(|t| t.replace("{repo_url}", &repo_url));
        self.refs = preset_ref_kinds(repo, Some(&repo_url));
        self.changelog.repo_url = Some(repo_url);
        self
    }

    pub fn is_package(&self) -> bool {
        self.package.is_some()
    }

    /// Package name. Panics on a shared config; check [`Config::is_package`] first.
    pub fn name(&self) -> &str {
        &self.package.as_ref().expect("not a package config").name
    }

    /// Look up a change type by canonical name or alias, case-insensitively.
    pub fn find_type(&self, name_or_alias: &str) -> Option<&ChangeType> {
        self.types.iter().find(|t| {
            t.name.eq_ignore_ascii_case(name_or_alias)
                || t.aliases.iter().any(|a| a.eq_ignore_ascii_case(name_or_alias))
        })
    }
}

/// The default change types: the Keep a Changelog categories.
pub fn default_types() -> Vec<ChangeType> {
    let t = |name: &str, heading: &str, aliases: &[&str], bump| ChangeType {
        name: name.to_string(),
        heading: heading.to_string(),
        aliases: aliases.iter().map(|a| a.to_string()).collect(),
        bump,
        hidden: false,
    };
    vec![
        t("added", "Added", &["feat", "feature"], Bump::Minor),
        t("changed", "Changed", &["change"], Bump::Patch),
        t("deprecated", "Deprecated", &["deprecation"], Bump::Minor),
        t("removed", "Removed", &["removal"], Bump::Major),
        t("fixed", "Fixed", &["fix", "bugfix"], Bump::Patch),
        t("security", "Security", &["sec"], Bump::Patch),
    ]
}

/// Link templates of a repository preset, with `{repo_url}` unsubstituted.
pub struct Preset {
    pub compare_url: Option<&'static str>,
    pub release_url: Option<&'static str>,
}

impl Preset {
    pub fn for_kind(kind: RepoKind) -> Preset {
        match kind {
            RepoKind::Github => Preset {
                compare_url: Some("{repo_url}/compare/{from}...{to}"),
                release_url: Some("{repo_url}/releases/tag/{tag}"),
            },
            RepoKind::Gitlab => Preset {
                compare_url: Some("{repo_url}/-/compare/{from}...{to}"),
                release_url: Some("{repo_url}/-/tags/{tag}"),
            },
            RepoKind::None => Preset { compare_url: None, release_url: None },
        }
    }
}

/// A preset reference kind with its templates unsubstituted.
pub(crate) struct RefTemplate {
    pub name: &'static str,
    pub matcher: &'static str,
    pub display: &'static str,
    pub url: &'static str,
}

const GITHUB_REFS: [RefTemplate; 2] = [
    RefTemplate { name: "issue", matcher: r"^(?<id>\d+)$", display: "#{id}", url: "{repo_url}/issues/{id}" },
    RefTemplate { name: "pr", matcher: r"^pr-(?<id>\d+)$", display: "#{id}", url: "{repo_url}/pull/{id}" },
];

const GITLAB_REFS: [RefTemplate; 2] = [
    RefTemplate { name: "issue", matcher: r"^(?<id>\d+)$", display: "#{id}", url: "{repo_url}/-/issues/{id}" },
    RefTemplate { name: "mr", matcher: r"^mr-(?<id>\d+)$", display: "!{id}", url: "{repo_url}/-/merge_requests/{id}" },
];

/// The reference kind templates of a repository preset.
pub(crate) fn preset_ref_templates(kind: RepoKind) -> &'static [RefTemplate] {
    match kind {
        RepoKind::Github => &GITHUB_REFS,
        RepoKind::Gitlab => &GITLAB_REFS,
        RepoKind::None => &[],
    }
}

/// The reference kinds of a repository preset. If `repo_url` is `None`, kinds
/// are still returned (so names are still recognised) but without URLs.
pub fn preset_ref_kinds(kind: RepoKind, repo_url: Option<&str>) -> Vec<RefKind> {
    preset_ref_templates(kind)
        .iter()
        .map(|t| RefKind {
            name: t.name.to_string(),
            matcher: Regex::new(t.matcher).expect("preset regex is valid"),
            display: t.display.to_string(),
            url: repo_url.map(|r| t.url.replace("{repo_url}", r.trim_end_matches('/'))),
        })
        .collect()
}

/// Find the config file that applies to `path`: the nearest `.vers.toml` or
/// `vers.toml` in `path` (if it's a directory) or any ancestor.
pub fn find_config_for(path: &Path) -> Result<Option<PathBuf>> {
    let path = resolve::absolute(path);
    // A path that doesn't exist (such as a deleted file in a diff) is treated
    // as a file, so the search starts at its parent.
    let start = if path.is_dir() {
        path.as_path()
    } else {
        match path.parent() {
            Some(parent) => parent,
            None => return Ok(None),
        }
    };
    Ok(start.ancestors().find_map(config_in_dir))
}

/// The config file in `dir` itself, honouring [`CONFIG_FILENAMES`] precedence.
fn config_in_dir(dir: &Path) -> Option<PathBuf> {
    CONFIG_FILENAMES.iter().map(|name| dir.join(name)).find(|p| p.is_file())
}

/// Load and fully resolve a config file: parse it, follow `extend`, apply the
/// repository preset (detecting it for `repo = "auto"`), fill in defaults and
/// make every path absolute. Checks `required_version` against this binary.
pub fn load(path: &Path) -> Result<Config> {
    resolve::load_config(path)
}

/// The config that applies to `path`. Errors if there is none.
pub fn load_for(path: &Path) -> Result<Config> {
    match find_config_for(path)? {
        Some(config) => load(&config),
        None => Err(no_config_error(path)),
    }
}

fn no_config_error(path: &Path) -> Error {
    Error::config(
        resolve::absolute(path),
        "no vers.toml or .vers.toml found here or in any parent directory; run `vers init` to create one",
    )
}

/// Every package config at or below `root`, respecting `.gitignore`, sorted by
/// path. Errors if two packages share a name.
pub fn discover_packages(root: &Path) -> Result<Vec<Config>> {
    let root = resolve::absolute(root);
    let mut dirs = std::collections::BTreeSet::new();
    let walker = ignore::WalkBuilder::new(&root)
        // `.vers.toml` is a hidden file; only `.git` itself is skipped.
        .hidden(false)
        .require_git(false)
        .filter_entry(|entry| entry.file_name() != ".git")
        .build();
    // Unreadable directories are skipped rather than failing the whole walk.
    for entry in walker.flatten() {
        let is_config = entry.file_name().to_str().is_some_and(|name| CONFIG_FILENAMES.contains(&name));
        if is_config && let Some(dir) = entry.path().parent() {
            dirs.insert(dir.to_path_buf());
        }
    }

    let mut packages = Vec::new();
    for dir in dirs {
        let Some(path) = config_in_dir(&dir) else { continue };
        let config = load(&path)?;
        if config.is_package() {
            packages.push(config);
        }
    }
    check_unique_names(&packages)?;
    Ok(packages)
}

fn check_unique_names(packages: &[Config]) -> Result<()> {
    for (i, a) in packages.iter().enumerate() {
        if let Some(b) = packages[i + 1..].iter().find(|b| b.name() == a.name()) {
            return Err(Error::config(
                &b.path,
                format!(
                    "package name `{}` is used by both {} and {}; package names must be unique, so set a different `name` in [package]",
                    a.name(),
                    a.path.display(),
                    b.path.display()
                ),
            ));
        }
    }
    Ok(())
}

/// Which packages a command acts on. See `docs/reference/cli.md#package-selection`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Selection {
    /// The package owning the current directory.
    Current,
    /// Packages by name, searched at or below the current directory.
    Named(Vec<String>),
    /// Every package at or below the current directory.
    All,
}

/// Resolve a [`Selection`] relative to `cwd`. With `config_override` (the
/// `--config` option), that file is used as the config for every path.
pub fn select_packages(cwd: &Path, selection: &Selection, config_override: Option<&Path>) -> Result<Vec<Config>> {
    let cwd = resolve::absolute(cwd);
    if let Some(path) = config_override {
        let config = load(&cwd.join(path))?;
        return select_from(vec![config], selection, &cwd, true);
    }
    match selection {
        Selection::Current => {
            let config = load_for(&cwd)?;
            if !config.is_package() {
                return Err(Error::config(
                    &config.path,
                    "the current directory isn't in a package: its nearest config is a shared config without a [package] table; run the command in a package directory, name the packages, or use --all",
                ));
            }
            Ok(vec![config])
        }
        Selection::Named(_) | Selection::All => {
            let packages = packages_at_or_below(&cwd)?;
            select_from(packages, selection, &cwd, false)
        }
    }
}

/// Packages below `cwd`, plus the package that owns `cwd` itself (its nearest
/// config may be in a parent directory, but `cwd` is still *in* it).
fn packages_at_or_below(cwd: &Path) -> Result<Vec<Config>> {
    let mut packages = discover_packages(cwd)?;
    if let Some(path) = find_config_for(cwd)?
        && !packages.iter().any(|p| p.path == path)
    {
        let owner = load(&path)?;
        if owner.is_package() {
            packages.insert(0, owner);
            check_unique_names(&packages)?;
        }
    }
    if packages.is_empty() && find_config_for(cwd)?.is_none() {
        return Err(no_config_error(cwd));
    }
    Ok(packages)
}

fn select_from(packages: Vec<Config>, selection: &Selection, cwd: &Path, from_override: bool) -> Result<Vec<Config>> {
    let packages: Vec<Config> = packages.into_iter().filter(Config::is_package).collect();
    match selection {
        Selection::Current | Selection::All => {
            if packages.is_empty() {
                return Err(if from_override {
                    Error::Usage("the config given with --config has no [package] table, so it isn't a package".into())
                } else {
                    Error::config(
                        cwd,
                        "no packages found at or below this directory; a package is a vers.toml with a [package] table",
                    )
                });
            }
            Ok(packages)
        }
        Selection::Named(names) => {
            let mut selected: Vec<Config> = Vec::new();
            let mut unknown: Vec<&str> = Vec::new();
            for name in names {
                if selected.iter().any(|c| c.name() == name) || unknown.contains(&name.as_str()) {
                    continue;
                }
                match packages.iter().find(|c| c.name() == name) {
                    Some(config) => selected.push(config.clone()),
                    None => unknown.push(name),
                }
            }
            if !unknown.is_empty() {
                let list = unknown.iter().map(|n| format!("`{n}`")).collect::<Vec<_>>().join(", ");
                let noun = if unknown.len() == 1 { "package" } else { "packages" };
                let available = if packages.is_empty() {
                    if from_override {
                        "the config given with --config isn't a package".to_string()
                    } else {
                        "no packages were found at or below the current directory".to_string()
                    }
                } else {
                    let names = packages.iter().map(Config::name).collect::<Vec<_>>().join(", ");
                    format!("available packages: {names}")
                };
                return Err(Error::Usage(format!("unknown {noun} {list}; {available}")));
            }
            Ok(selected)
        }
    }
}

#[cfg(test)]
mod tests;
