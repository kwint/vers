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

use crate::error::Result;
use crate::version::Bump;

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

/// The reference kinds of a repository preset. If `repo_url` is `None`, kinds
/// are still returned (so names are still recognised) but without URLs.
pub fn preset_ref_kinds(kind: RepoKind, repo_url: Option<&str>) -> Vec<RefKind> {
    let k = |name: &str, re: &str, display: &str, url: &str| RefKind {
        name: name.to_string(),
        matcher: Regex::new(re).expect("preset regex is valid"),
        display: display.to_string(),
        url: repo_url.map(|r| url.replace("{repo_url}", r.trim_end_matches('/'))),
    };
    match kind {
        RepoKind::Github => vec![
            k("issue", r"^(?<id>\d+)$", "#{id}", "{repo_url}/issues/{id}"),
            k("pr", r"^pr-(?<id>\d+)$", "#{id}", "{repo_url}/pull/{id}"),
        ],
        RepoKind::Gitlab => vec![
            k("issue", r"^(?<id>\d+)$", "#{id}", "{repo_url}/-/issues/{id}"),
            k("mr", r"^mr-(?<id>\d+)$", "!{id}", "{repo_url}/-/merge_requests/{id}"),
        ],
        RepoKind::None => Vec::new(),
    }
}

/// Find the config file that applies to `path`: the nearest `.vers.toml` or
/// `vers.toml` in `path` (if it's a directory) or any ancestor.
pub fn find_config_for(path: &Path) -> Result<Option<PathBuf>> {
    let _ = path;
    todo!("phase 1: config")
}

/// Load and fully resolve a config file: parse it, follow `extend`, apply the
/// repository preset (detecting it for `repo = "auto"`), fill in defaults and
/// make every path absolute. Checks `required_version` against this binary.
pub fn load(path: &Path) -> Result<Config> {
    let _ = path;
    todo!("phase 1: config")
}

/// The config that applies to `path`. Errors if there is none.
pub fn load_for(path: &Path) -> Result<Config> {
    let _ = path;
    todo!("phase 1: config")
}

/// Every package config at or below `root`, respecting `.gitignore`, sorted by
/// path. Errors if two packages share a name.
pub fn discover_packages(root: &Path) -> Result<Vec<Config>> {
    let _ = root;
    todo!("phase 1: config")
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
    let _ = (cwd, selection, config_override);
    todo!("phase 1: config")
}
